//! Backup locations and rollback - the same folders and file names
//! ZZZ-Mod-Fixer's `Ini.save()` uses, so a file fixed by either tool can be
//! restored by the other.
//!
//! Upstream search order for the backup folder, from the INI upwards:
//! `<dir>/Data/ZZMI`, `<dir>` when it is itself named `ZZMI`, then
//! `<dir>/ZZMI`; if none of those exist anywhere above the INI, the drive roots
//! are tried (`XXMI\Data\ZZMI`, `Data\ZZMI`, `XXMI\ZZMI`,
//! `XXMI Launcher\ZZMI`, `ZZMI`). The winner gets `(.ini) Caches` appended;
//! with no winner the backups land next to the INI.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::ini_parser::text::write_atomic;

/// Folder inside a resolved `ZZMI` root where disabled INI backups are kept.
pub const CACHE_DIR: &str = "(.ini) Caches";

/// Backup file name prefix, as upstream writes it.
pub const BACKUP_PREFIX: &str = "DISABLED_BACKUP_";

/// Relative roots tried under each drive letter, in upstream's order.
const DRIVE_ROOTS: [&[&str]; 5] = [
    &["XXMI", "Data", "ZZMI"],
    &["Data", "ZZMI"],
    &["XXMI", "ZZMI"],
    &["XXMI Launcher", "ZZMI"],
    &["ZZMI"],
];

/// Candidate `ZZMI` roots for one INI, nearest first.
fn roots(ini: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut current = ini.parent().map(Path::to_path_buf);

    while let Some(dir) = current {
        roots.push(dir.join("Data").join("ZZMI"));
        if dir
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("ZZMI"))
        {
            roots.push(dir.clone());
        }
        roots.push(dir.join("ZZMI"));
        current = dir.parent().map(Path::to_path_buf);
    }

    for letter in b'A'..=b'Z' {
        let drive = PathBuf::from(format!("{}:\\", letter as char));
        if !drive.exists() {
            continue;
        }
        for parts in DRIVE_ROOTS {
            let mut root = drive.clone();
            for part in parts {
                root.push(part);
            }
            roots.push(root);
        }
    }

    roots
}

/// The folder this INI's backup belongs in. Falls back to the INI's own folder,
/// exactly like upstream when no `ZZMI` root is found.
pub fn dir_for(ini: &Path) -> PathBuf {
    for root in roots(ini) {
        if root.is_dir() {
            return root.join(CACHE_DIR);
        }
    }

    ini.parent().unwrap_or_else(|| Path::new(".")).to_path_buf()
}

/// Upstream strips everything from the first `.ini` in the file name, so
/// `merged.ini` becomes `merged`.
pub fn basename(file_name: &str) -> String {
    let lower = file_name.to_ascii_lowercase();
    match lower.find(".ini") {
        Some(index) => file_name[..index].to_string(),
        None => file_name.to_string(),
    }
}

/// `DISABLED_BACKUP_<unixtime>_<basename>.txt` - where the original INI is moved.
pub fn backup_name(file_name: &str, timestamp: u64) -> String {
    format!(
        "{BACKUP_PREFIX}{timestamp}_{}.txt",
        basename(file_name)
    )
}

/// Move `ini` to its backup location. Returns the backup path.
///
/// A rename is used first (same volume, atomic). Across volumes the content is
/// copied instead; if that fails too, the INI is left alone and the error is
/// returned - the caller must not write without a backup it asked for.
pub fn stash(ini: &Path, timestamp: u64) -> AppResult<PathBuf> {
    let mut dir = dir_for(ini);
    if let Err(error) = fs::create_dir_all(&dir) {
        log::warn!(
            "Could not create backup folder {}: {error}",
            dir.display()
        );
        dir = ini
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
    }

    let file_name = ini
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();

    // Two INIs with the same name in one backup folder (a mod shipping
    // `body/merged.ini` and `hair/merged.ini`) would otherwise overwrite each
    // other's backup within the same second. Upstream loses the first one.
    let mut stamp = timestamp;
    let mut target = dir.join(backup_name(&file_name, stamp));
    for _ in 0..1_000 {
        if !target.exists() {
            break;
        }
        stamp += 1;
        target = dir.join(backup_name(&file_name, stamp));
    }

    match fs::rename(ini, &target) {
        Ok(()) => Ok(target),
        Err(rename_error) => {
            log::warn!(
                "Could not move {} into {} ({rename_error}); copying instead",
                ini.display(),
                target.display()
            );
            fs::copy(ini, &target)?;
            fs::remove_file(ini)?;
            Ok(target)
        }
    }
}

/// Newest backup for `ini`, by the timestamp in its name. Looks in the resolved
/// backup folder and next to the INI (both places upstream writes).
pub fn newest_backup(ini: &Path) -> Option<PathBuf> {
    let file_name = ini.file_name()?.to_string_lossy().to_string();
    let stem = basename(&file_name);
    let suffixes = [
        format!("_{stem}.txt"),
        format!(".{stem}.txt"),
    ];

    let mut search = vec![dir_for(ini)];
    if let Some(parent) = ini.parent() {
        if !search.iter().any(|dir| dir == parent) {
            search.push(parent.to_path_buf());
        }
    }

    let mut best: Option<(u64, PathBuf)> = None;
    for dir in search {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with(BACKUP_PREFIX) || !suffixes.iter().any(|s| name.ends_with(s)) {
                continue;
            }

            let stamp = name
                .strip_prefix(BACKUP_PREFIX)
                .and_then(|rest| rest.split(['_', '.']).next())
                .and_then(|digits| digits.parse::<u64>().ok())
                .unwrap_or(0);

            if best.as_ref().is_none_or(|(best_stamp, _)| stamp >= *best_stamp) {
                best = Some((stamp, entry.path()));
            }
        }
    }

    best.map(|(_, path)| path)
}

/// Put a backup back where the INI was and consume the backup.
pub fn restore(ini: &Path) -> AppResult<PathBuf> {
    let backup = newest_backup(ini).ok_or_else(|| {
        AppError::IniNotFound(format!(
            "no {}...{} backup found for {}",
            BACKUP_PREFIX,
            basename(
                &ini
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default()
            ),
            ini.display()
        ))
    })?;

    restore_from(ini, &backup)
}

/// Put one specific backup back over the INI and consume it. `restore` picks
/// the newest backup by name; the fixer history restores the exact backup a
/// recorded fix wrote, which is the only way to undo an older fix of a file
/// that was fixed twice.
pub fn restore_from(ini: &Path, backup: &Path) -> AppResult<PathBuf> {
    if !backup.is_file() {
        return Err(AppError::IniNotFound(format!(
            "the backup {} is no longer on disk",
            backup.display()
        )));
    }

    let bytes = fs::read(backup)?;
    write_atomic(ini, &bytes)?;
    fs::remove_file(backup)?;

    Ok(backup.to_path_buf())
}

/// Unix seconds, the unit upstream stamps backups with.
pub fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zzmm-fixer-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn backup_names_match_upstream() {
        assert_eq!(backup_name("merged.ini", 1_700_000_000), "DISABLED_BACKUP_1700000000_merged.txt");
        assert_eq!(backup_name("0Vivian.ini", 42), "DISABLED_BACKUP_42_0Vivian.txt");
        // Upstream splits on the first `.ini`, wherever it appears.
        assert_eq!(basename("a.ini.bak"), "a");
        assert_eq!(basename("no-extension"), "no-extension");
    }

    /// Upstream also writes `DISABLED_BACKUP_<time>.<basename>.txt` beside the
    /// INI when it cannot move the original (`Ini.save`'s copy fallback). This
    /// port never writes that spelling - it copies the content instead - but a
    /// rollback must still find and consume one left by the Python tool.
    #[test]
    fn rollback_finds_an_upstream_emergency_copy() {
        let root = scratch("emergency");
        let mods = root.join("Mods").join("EllenMod");
        fs::create_dir_all(&mods).expect("mods");
        let ini = mods.join("merged.ini");
        let original = b"[TextureOverrideX]\r\nhash = 3c8697e8\r\n";
        fs::write(&ini, original).expect("seed");

        let emergency = mods.join("DISABLED_BACKUP_7.merged.txt");
        fs::write(&emergency, original).expect("emergency copy");
        fs::write(&ini, b"[TextureOverrideX]\r\nhash = 67821d9d\r\n").expect("rewrite");

        let restored_from = restore(&ini).expect("restore");
        assert_eq!(restored_from, emergency);
        assert_eq!(fs::read(&ini).expect("read"), original);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn backup_folder_is_the_zzmi_cache_next_to_the_mods() {
        let root = scratch("caches");
        let mods = root.join("XXMI Launcher").join("ZZMI").join("Mods").join("EllenMod");
        fs::create_dir_all(&mods).expect("mods");
        fs::create_dir_all(root.join("XXMI Launcher").join("ZZMI").join("Data").join("ZZMI"))
            .expect("data");

        let ini = mods.join("merged.ini");
        fs::write(&ini, "[TextureOverrideX]\nhash = 1\n").expect("ini");

        // `…/ZZMI/Data/ZZMI` is found by walking up from the INI, so the backup
        // folder is the cache inside it - the same choice the Python tool makes.
        let expected = root
            .join("XXMI Launcher")
            .join("ZZMI")
            .join("Data")
            .join("ZZMI")
            .join(CACHE_DIR);
        assert_eq!(dir_for(&ini), expected);

        let stored = stash(&ini, 1_700_000_000).expect("stash");
        assert_eq!(stored, expected.join("DISABLED_BACKUP_1700000000_merged.txt"));
        assert!(stored.is_file());
        assert!(!ini.exists(), "the original is moved, not copied");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn backup_folder_falls_back_to_the_mod_folder() {
        let root = scratch("fallback");
        let mods = root.join("Somewhere").join("Mods").join("EllenMod");
        fs::create_dir_all(&mods).expect("mods");
        let ini = mods.join("Ellen.ini");
        fs::write(&ini, "hash = 1\n").expect("ini");

        // No `ZZMI` folder above it and (unless the machine really has one) no
        // drive-root layout either: backups land beside the INI.
        assert_eq!(dir_for(&ini), mods);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn backup_folder_finds_a_sibling_zzmi_root() {
        // `<root>/ZZMI` next to the Mods folder is case B of upstream's search.
        let root = scratch("sibling");
        let mods = root.join("Mods").join("EllenMod");
        fs::create_dir_all(&mods).expect("mods");
        fs::create_dir_all(root.join("ZZMI")).expect("zzmi root");
        let ini = mods.join("merged.ini");
        fs::write(&ini, "hash = 1\n").expect("ini");

        assert_eq!(dir_for(&ini), root.join("ZZMI").join(CACHE_DIR));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rollback_restores_the_original_bytes_and_consumes_the_backup() {
        let root = scratch("rollback");
        let mods = root.join("Mods").join("EllenMod");
        fs::create_dir_all(&mods).expect("mods");
        let ini = mods.join("Ellen.ini");
        let original = b"[TextureOverrideX]\r\nhash = 3c8697e8\r\n; keep me\r\n";
        fs::write(&ini, original).expect("seed");

        let stored = stash(&ini, 100).expect("stash");
        fs::write(&ini, b"[TextureOverrideX]\r\nhash = 67821d9d\r\n; keep me\r\n").expect("rewrite");

        let restored_from = restore(&ini).expect("restore");
        assert_eq!(restored_from, stored);
        assert_eq!(fs::read(&ini).expect("read"), original);
        assert!(!stored.exists(), "the backup is consumed by the rollback");
        assert!(restore(&ini).is_err(), "nothing left to restore");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rollback_picks_the_newest_backup() {
        let root = scratch("newest");
        let mods = root.join("Mods").join("EllenMod");
        fs::create_dir_all(&mods).expect("mods");
        let ini = mods.join("Ellen.ini");
        fs::write(&ini, b"new\n").expect("seed");

        let older = mods.join(backup_name("Ellen.ini", 100));
        let newer = mods.join(backup_name("Ellen.ini", 200));
        fs::write(&older, b"older\n").expect("older");
        fs::write(&newer, b"newer\n").expect("newer");

        assert_eq!(newest_backup(&ini), Some(newer));

        fs::remove_dir_all(&root).ok();
    }

    /// The fixer history restores the backup a *specific* fix wrote, so an
    /// older fix of the same file can still be undone after a newer one.
    #[test]
    fn restore_from_puts_back_the_named_backup_even_when_a_newer_one_exists() {
        let root = scratch("exact");
        let mods = root.join("Mods").join("EllenMod");
        fs::create_dir_all(&mods).expect("mods");
        let ini = mods.join("Ellen.ini");
        fs::write(&ini, b"fixed twice\n").expect("seed");

        let first = mods.join(backup_name("Ellen.ini", 100));
        let second = mods.join(backup_name("Ellen.ini", 200));
        fs::write(&first, b"pristine\n").expect("first");
        fs::write(&second, b"after the first fix\n").expect("second");

        let restored_from = restore_from(&ini, &first).expect("restore");
        assert_eq!(restored_from, first);
        assert_eq!(fs::read(&ini).expect("read"), b"pristine\n");
        assert!(!first.exists(), "only the named backup is consumed");
        assert!(second.exists(), "the newer backup is left alone");

        assert!(restore_from(&ini, &first).is_err(), "a consumed backup cannot restore again");

        fs::remove_dir_all(&root).ok();
    }
}
