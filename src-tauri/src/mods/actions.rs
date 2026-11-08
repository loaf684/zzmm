use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::error::{AppError, AppResult};
use crate::mods::metadata;
use crate::mods::model::{ModDetails, ModStateFailure, ModStateUpdate};
use crate::mods::scanner::{split_disabled, DISABLED_PREFIX};
use crate::util::iso_from_unix;

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];
const MAX_PREVIEW_BYTES: u64 = 20 * 1024 * 1024;
/// Hard stop for the recursive size walk so a pathological folder cannot hang
/// the detail panel.
const MAX_SIZE_ENTRIES: usize = 60_000;

/// Reject anything that could escape the configured Mods folder. Only a single
/// directory name is ever accepted from the frontend.
pub fn validate_folder_name(folder_name: &str) -> AppResult<()> {
    let invalid = folder_name.is_empty()
        || folder_name == "."
        || folder_name == ".."
        || folder_name.trim() != folder_name
        || folder_name.len() > 200
        || folder_name.chars().any(|character| {
            matches!(
                character,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            )
        })
        || folder_name.chars().any(|character| character.is_control());

    if invalid {
        return Err(AppError::InvalidModFolder(folder_name.to_string()));
    }

    Ok(())
}

/// Resolve an existing mod folder and ensure it is a direct child of `mods_dir`.
pub fn mod_dir(mods_dir: &Path, folder_name: &str) -> AppResult<PathBuf> {
    validate_folder_name(folder_name)?;

    let path = mods_dir.join(folder_name);
    if !path.is_dir() {
        return Err(AppError::ModFolderMissing(folder_name.to_string()));
    }
    if path.parent() != Some(mods_dir) {
        return Err(AppError::InvalidModFolder(folder_name.to_string()));
    }

    Ok(path)
}

/// Toggle the ZZMI `DISABLED_` prefix. Renames never cross the Mods folder.
pub fn set_enabled(mods_dir: &Path, folder_name: &str, enabled: bool) -> AppResult<()> {
    let source = mod_dir(mods_dir, folder_name)?;
    let (is_disabled, base_name) = split_disabled(folder_name);

    if base_name.trim().is_empty() {
        return Err(AppError::InvalidModFolder(folder_name.to_string()));
    }

    // Already in the requested state.
    let currently_enabled = !is_disabled;
    if currently_enabled == enabled {
        return Ok(());
    }

    let target_name = if enabled {
        base_name
    } else {
        format!("{DISABLED_PREFIX}{base_name}")
    };
    let target = mods_dir.join(&target_name);

    if target.exists() {
        return Err(AppError::ModFolderExists(target_name));
    }

    fs::rename(&source, &target)?;
    log::info!(
        "Mod {} -> {}",
        folder_name,
        target.file_name().unwrap_or_default().to_string_lossy()
    );

    Ok(())
}

pub fn delete(mods_dir: &Path, folder_name: &str) -> AppResult<()> {
    let path = mod_dir(mods_dir, folder_name)?;
    let metadata = fs::symlink_metadata(&path)?;

    // Never follow a link when deleting: remove the link itself instead.
    if metadata.file_type().is_symlink() {
        fs::remove_file(&path)?;
    } else if metadata.is_dir() {
        fs::remove_dir_all(&path)?;
    } else {
        return Err(AppError::ModNotDirectory(folder_name.to_string()));
    }

    log::info!("Deleted mod folder {}", path.display());
    Ok(())
}

pub fn open_in_explorer(path: &Path) -> AppResult<()> {
    #[cfg(target_os = "windows")]
    let program = "explorer.exe";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";

    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map_err(|error| AppError::Explorer(error.to_string()))?;

    Ok(())
}

/// Open an http(s) URL in the user's default browser. Tauri windows cannot
/// navigate to arbitrary sites, so the OS handler is invoked directly and the
/// input is restricted to http(s) to avoid protocol-handler abuse.
pub fn open_url(url: &str) -> AppResult<()> {
    let trimmed = url.trim();
    let valid = (trimmed.starts_with("https://") || trimmed.starts_with("http://"))
        && trimmed.len() <= 2048
        && !trimmed
            .chars()
            .any(|character| character.is_whitespace() || character.is_control());

    if !valid {
        return Err(AppError::InvalidUrl(url.to_string()));
    }

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command
    };

    command
        .arg(trimmed)
        .spawn()
        .map_err(|error| AppError::Explorer(error.to_string()))?;

    Ok(())
}

/// Everything the detail panel needs beyond the scan summary.
///
/// Runs a bounded recursive size walk, so callers must keep it off the UI
/// thread.
pub fn details(mods_dir: &Path, folder_name: &str) -> AppResult<ModDetails> {
    let dir = mod_dir(mods_dir, folder_name)?;
    let meta = metadata::read(&dir);

    let installed_at = meta
        .as_ref()
        .and_then(|meta| meta.installed_at.clone())
        .or_else(|| folder_created_at(&dir));

    Ok(ModDetails {
        folder_name: folder_name.to_string(),
        previews: preview_images(&dir),
        folder_size: dir_size(&dir),
        installed_at,
        notes: meta.and_then(|meta| meta.notes),
    })
}

fn folder_created_at(dir: &Path) -> Option<String> {
    let created = fs::metadata(dir).ok()?.created().ok()?;
    let seconds = created.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    Some(iso_from_unix(seconds))
}

fn image_extension(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
    IMAGE_EXTENSIONS
        .contains(&extension.as_str())
        .then_some(extension)
}

/// Root-level `preview*` images, in the order the carousel should show them.
/// Nested images are textures, so they are deliberately ignored.
fn preview_images(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut images: Vec<(String, String)> = entries
        .flatten()
        .filter_map(|entry| {
            if !entry.file_type().ok()?.is_file() {
                return None;
            }

            let path = entry.path();
            image_extension(&path)?;

            let stem = path.file_stem()?.to_string_lossy().to_ascii_lowercase();
            if !stem.starts_with("preview") {
                return None;
            }

            if entry.metadata().ok()?.len() > MAX_PREVIEW_BYTES {
                return None;
            }

            Some((stem, path.to_string_lossy().to_string()))
        })
        .collect();

    images.sort_by(|a, b| a.0.cmp(&b.0));
    images.into_iter().map(|(_, path)| path).collect()
}

/// Total size of regular files, skipping symlinks and stopping after a fixed
/// number of entries.
fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let mut visited = 0usize;
    let mut pending = vec![dir.to_path_buf()];

    while let Some(current) = pending.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };

        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_SIZE_ENTRIES {
                return total;
            }

            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                if let Ok(metadata) = entry.metadata() {
                    total = total.saturating_add(metadata.len());
                }
            }
        }
    }

    total
}

/// Rename a mod folder while preserving its `DISABLED_` state. A leading
/// `DISABLED_` in the new name is stripped so enabled mods cannot be renamed
/// into a disabled state by accident (use the toggle for that).
pub fn rename(mods_dir: &Path, folder_name: &str, new_name: &str) -> AppResult<String> {
    let source = mod_dir(mods_dir, folder_name)?;
    let (was_disabled, old_base) = split_disabled(folder_name);

    let (_, stripped) = split_disabled(new_name.trim());
    let base = stripped.trim().to_string();
    validate_folder_name(&base)?;

    if base == old_base {
        return Ok(folder_name.to_string());
    }

    let target_name = if was_disabled {
        format!("{DISABLED_PREFIX}{base}")
    } else {
        base.clone()
    };
    let target = mods_dir.join(&target_name);

    if target.exists() {
        return Err(AppError::ModFolderExists(target_name));
    }

    fs::rename(&source, &target)?;

    // Keep the visible title in sync for linked mods; a failure here is not
    // fatal because the folder rename already succeeded.
    if let Some(mut meta) = metadata::read(&target) {
        meta.name = Some(base);
        if let Err(error) = metadata::write(&target, &meta) {
            log::warn!("Could not update .zzmm.json after rename: {error}");
        }
    }

    log::info!("Renamed mod folder {folder_name} -> {target_name}");
    Ok(target_name)
}

/// Persist user notes in the mod's sidecar. Invalid existing JSON is never
/// overwritten, so hand-edited metadata cannot be lost.
pub fn set_notes(mods_dir: &Path, folder_name: &str, notes: &str) -> AppResult<()> {
    let dir = mod_dir(mods_dir, folder_name)?;
    let existing = metadata::read(&dir);
    let metadata_path = dir.join(metadata::METADATA_FILE);

    let mut meta = match (existing, metadata_path.exists()) {
        (Some(meta), _) => meta,
        (None, true) => {
            return Err(AppError::InvalidMetadata(
                metadata_path.display().to_string(),
            ))
        }
        (None, false) => metadata::ModMetadata::default(),
    };

    let trimmed = notes.trim();
    meta.notes = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    };

    metadata::write(&dir, &meta)?;
    Ok(())
}

/// Persist a user-chosen character name in the mod's sidecar. An empty value
/// clears the override so the scanner falls back to the folder-name heuristic.
pub fn set_character(mods_dir: &Path, folder_name: &str, character: Option<&str>) -> AppResult<()> {
    let dir = mod_dir(mods_dir, folder_name)?;
    let existing = metadata::read(&dir);
    let metadata_path = dir.join(metadata::METADATA_FILE);

    let mut meta = match (existing, metadata_path.exists()) {
        (Some(meta), _) => meta,
        (None, true) => {
            return Err(AppError::InvalidMetadata(
                metadata_path.display().to_string(),
            ))
        }
        (None, false) => metadata::ModMetadata::default(),
    };

    let trimmed = character.unwrap_or("").trim();
    if trimmed.len() > 80 || trimmed.chars().any(char::is_control) {
        return Err(AppError::InvalidCharacter(trimmed.to_string()));
    }

    meta.character = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    };

    metadata::write(&dir, &meta)?;
    Ok(())
}

/// Apply many enable/disable changes in one batch. A failure on one mod is
/// reported without aborting the rest, so a single broken folder cannot block a
/// loadout switch.
pub fn apply_states(
    mods_dir: &Path,
    updates: &[ModStateUpdate],
) -> (Vec<String>, Vec<ModStateFailure>) {
    let mut changed = Vec::new();
    let mut failed = Vec::new();

    for update in updates {
        // A missing folder is a failure unless a previous batch already renamed
        // it to the requested state; callers may keep the stale folder name.
        if let Err(error) = mod_dir(mods_dir, &update.folder_name) {
            let (_, base_name) = split_disabled(&update.folder_name);
            let expected = if update.enabled {
                base_name
            } else {
                format!("{DISABLED_PREFIX}{base_name}")
            };

            if !mods_dir.join(expected).is_dir() {
                failed.push(ModStateFailure {
                    folder_name: update.folder_name.clone(),
                    message: error.to_string(),
                });
            }
            continue;
        }

        let (is_disabled, _) = split_disabled(&update.folder_name);
        let exists = mods_dir.join(&update.folder_name).is_dir();
        if exists && (!is_disabled) == update.enabled {
            continue;
        }

        match set_enabled(mods_dir, &update.folder_name, update.enabled) {
            Ok(()) => changed.push(update.folder_name.clone()),
            Err(error) => failed.push(ModStateFailure {
                folder_name: update.folder_name.clone(),
                message: error.to_string(),
            }),
        }
    }

    (changed, failed)
}

/// Copy a user-chosen image into the mod folder as `preview.<ext>` and remove
/// the previous `preview.*` image so the thumbnail stays deterministic.
pub fn set_preview(mods_dir: &Path, folder_name: &str, source_path: &str) -> AppResult<()> {
    let dir = mod_dir(mods_dir, folder_name)?;
    let source = Path::new(source_path);

    if !source.is_file() {
        return Err(AppError::InvalidPreview(
            "The selected file no longer exists.".to_string(),
        ));
    }

    let extension = image_extension(source).ok_or_else(|| {
        AppError::InvalidPreview("Choose a PNG, JPG, WEBP or GIF image.".to_string())
    })?;

    let size = fs::metadata(source)?.len();
    if size > MAX_PREVIEW_BYTES {
        return Err(AppError::InvalidPreview(
            "Preview images must be 20 MB or smaller.".to_string(),
        ));
    }

    let target = dir.join(format!("preview.{extension}"));
    let temp = dir.join(".zzmm-preview.tmp");

    // Copy first so a failed copy never destroys the current preview.
    fs::copy(source, &temp)?;

    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_preview = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().eq_ignore_ascii_case("preview"))
                .unwrap_or(false);

            if is_preview && image_extension(&path).is_some() && path != target {
                if let Err(error) = fs::remove_file(&path) {
                    log::warn!("Could not replace old preview {}: {error}", path.display());
                }
            }
        }
    }

    // `rename` does not overwrite on Windows, so drop the old target first.
    if target.exists() {
        fs::remove_file(&target)?;
    }

    fs::rename(&temp, &target)?;
    log::info!("Updated preview for {folder_name}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("zzmm-actions-test-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn rejects_names_that_can_escape_the_mods_folder() {
        assert!(validate_folder_name("../escape").is_err());
        assert!(validate_folder_name("a\\b").is_err());
        assert!(validate_folder_name("a/b").is_err());
        assert!(validate_folder_name("").is_err());
        assert!(validate_folder_name("C:evil").is_err());
        assert!(validate_folder_name("Normal_Mod").is_ok());
    }

    #[test]
    fn toggles_prefix_and_reports_collisions() {
        let root = temp_dir();
        fs::create_dir_all(root.join("Foo")).expect("mod folder");

        set_enabled(&root, "Foo", false).expect("disable");
        assert!(root.join("DISABLED_Foo").is_dir());
        assert!(!root.join("Foo").exists());

        // Recreating the enabled folder must block the re-enable.
        fs::create_dir_all(root.join("Foo")).expect("collision folder");
        let error = set_enabled(&root, "DISABLED_Foo", true).expect_err("collision");
        assert!(matches!(error, AppError::ModFolderExists(_)));

        // Disabling an already-disabled mod is a no-op.
        set_enabled(&root, "DISABLED_Foo", false).expect("no-op");

        delete(&root, "DISABLED_Foo").expect("delete");
        delete(&root, "Foo").expect("delete");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn renames_while_preserving_the_disabled_state() {
        let root = temp_dir();
        fs::create_dir_all(root.join("DISABLED_Foo")).expect("mod folder");
        metadata::write(
            &root.join("DISABLED_Foo"),
            &metadata::ModMetadata {
                name: Some("Old title".into()),
                ..metadata::ModMetadata::default()
            },
        )
        .expect("metadata");

        let renamed = rename(&root, "DISABLED_Foo", "Bar").expect("rename disabled");
        assert_eq!(renamed, "DISABLED_Bar");
        assert!(root.join("DISABLED_Bar").is_dir());
        assert_eq!(
            metadata::read(&root.join("DISABLED_Bar"))
                .and_then(|meta| meta.name)
                .as_deref(),
            Some("Bar")
        );

        // Enabled mods stay enabled, and a typed prefix is ignored.
        fs::create_dir_all(root.join("Enabled Mod")).expect("enabled folder");
        let renamed = rename(&root, "Enabled Mod", "DISABLED_Sneaky").expect("rename enabled");
        assert_eq!(renamed, "Sneaky");
        assert!(root.join("Sneaky").is_dir());

        fs::create_dir_all(root.join("DISABLED_Taken")).expect("collision");
        let error = rename(&root, "DISABLED_Bar", "Taken").expect_err("collision");
        assert!(matches!(error, AppError::ModFolderExists(_)));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn saves_and_clears_notes_without_clobbering_invalid_metadata() {
        let root = temp_dir();
        fs::create_dir_all(root.join("Notes")).expect("mod folder");

        set_notes(&root, "Notes", "  remember this  ").expect("save notes");
        assert_eq!(
            metadata::read(&root.join("Notes"))
                .and_then(|meta| meta.notes)
                .as_deref(),
            Some("remember this")
        );

        set_notes(&root, "Notes", "   ").expect("clear notes");
        assert!(metadata::read(&root.join("Notes"))
            .and_then(|meta| meta.notes)
            .is_none());

        fs::create_dir_all(root.join("Broken")).expect("broken folder");
        fs::write(root.join("Broken/.zzmm.json"), "{not json").expect("write");
        let error = set_notes(&root, "Broken", "x").expect_err("invalid metadata");
        assert!(matches!(error, AppError::InvalidMetadata(_)));
        assert_eq!(
            fs::read_to_string(root.join("Broken/.zzmm.json")).expect("still there"),
            "{not json"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn saves_and_clears_character_overrides() {
        let root = temp_dir();
        fs::create_dir_all(root.join("Chars")).expect("mod folder");

        set_character(&root, "Chars", Some("  Ellen Joe  ")).expect("set character");
        assert_eq!(
            metadata::read(&root.join("Chars"))
                .and_then(|meta| meta.character)
                .as_deref(),
            Some("Ellen Joe")
        );

        set_character(&root, "Chars", Some("   ")).expect("clear character");
        assert!(metadata::read(&root.join("Chars"))
            .and_then(|meta| meta.character)
            .is_none());

        let error = set_character(&root, "Chars", Some("bad\u{7}name"))
            .expect_err("control characters are rejected");
        assert!(matches!(error, AppError::InvalidCharacter(_)));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn applies_batch_states_and_reports_failures() {
        let root = temp_dir();
        fs::create_dir_all(root.join("On")).expect("enabled folder");
        fs::create_dir_all(root.join("DISABLED_Off")).expect("disabled folder");

        let (changed, failed) = apply_states(
            &root,
            &[
                ModStateUpdate {
                    folder_name: "On".into(),
                    enabled: false,
                },
                ModStateUpdate {
                    folder_name: "DISABLED_Off".into(),
                    enabled: true,
                },
                ModStateUpdate {
                    folder_name: "Missing".into(),
                    enabled: true,
                },
            ],
        );

        assert_eq!(changed.len(), 2);
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].folder_name, "Missing");
        assert!(root.join("DISABLED_On").is_dir());
        assert!(root.join("Off").is_dir());

        // Already in the requested state: no rename, no failure.
        let (changed, failed) = apply_states(
            &root,
            &[ModStateUpdate {
                folder_name: "On".into(),
                enabled: false,
            }],
        );
        assert!(changed.is_empty());
        assert!(failed.is_empty());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn replaces_previews_and_measures_the_folder() {
        let root = temp_dir();
        let mod_dir = root.join("Preview Mod");
        fs::create_dir_all(&mod_dir).expect("mod folder");
        fs::write(mod_dir.join("preview.png"), b"old").expect("old preview");
        fs::write(mod_dir.join("notes.txt"), b"12345").expect("notes file");

        let source = root.join("new.jpg");
        fs::write(&source, b"newimage").expect("new preview");

        set_preview(&root, "Preview Mod", &source.to_string_lossy()).expect("set preview");
        assert!(!mod_dir.join("preview.png").exists());
        assert_eq!(
            fs::read(mod_dir.join("preview.jpg")).expect("new preview bytes"),
            b"newimage"
        );

        let info = details(&root, "Preview Mod").expect("details");
        assert_eq!(info.previews.len(), 1);
        assert!(info.previews[0].ends_with("preview.jpg"));
        assert_eq!(info.folder_size, 13);
        assert!(info.installed_at.is_some());

        let text_file = mod_dir.join("notes.txt").to_string_lossy().to_string();
        let error = set_preview(&root, "Preview Mod", &text_file).expect_err("not an image");
        assert!(matches!(error, AppError::InvalidPreview(_)));

        fs::remove_dir_all(&root).ok();
    }
}
