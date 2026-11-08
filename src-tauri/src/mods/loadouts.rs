//! Saved mod loadouts and safety snapshots.
//!
//! A loadout records the enabled state of every mod that existed when it was
//! saved. Applying one only touches mods that are still present, so installing
//! a new mod after saving never silently disables it. Batch operations write a
//! timestamped snapshot of the current state before renaming any folder.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::mods::model::{ModScanResult, ModStateFailure};
use crate::mods::scanner::split_disabled;
use crate::util::iso_utc_now;

pub const LOADOUTS_FILE: &str = "loadouts.json";
const SCHEMA_VERSION: u32 = 1;
const MAX_NAME_LENGTH: usize = 80;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadoutEntry {
    pub base_name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Loadout {
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub entries: Vec<LoadoutEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadoutSummary {
    pub name: String,
    pub entry_count: u32,
    pub enabled_count: u32,
    pub created_at: String,
    pub updated_at: String,
}

/// Current on-disk state of one mod folder. The folder name carries the
/// `DISABLED_` prefix, which is what batch operations rename.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModStateSnapshot {
    pub folder_name: String,
    pub base_name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadoutApplyResult {
    pub scan: ModScanResult,
    pub changed: Vec<String>,
    /// Loadout entries whose mod folder no longer exists.
    pub missing: Vec<String>,
    pub failed: Vec<ModStateFailure>,
    pub backup_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoadoutFile {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(default)]
    loadouts: Vec<Loadout>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StateBackup {
    created_at: String,
    mods_dir: String,
    states: Vec<ModStateSnapshot>,
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

impl Default for LoadoutFile {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            loadouts: Vec::new(),
        }
    }
}

fn loadouts_path(config_dir: &Path) -> PathBuf {
    config_dir.join(LOADOUTS_FILE)
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

/// Validate a user-facing loadout name. Names are the unique key, so trimming
/// and a length cap keep the UI and file predictable.
pub fn validate_name(name: &str) -> AppResult<String> {
    let trimmed = name.trim();

    if trimmed.is_empty()
        || trimmed.len() > MAX_NAME_LENGTH
        || trimmed.chars().any(char::is_control)
    {
        return Err(AppError::InvalidLoadout(name.to_string()));
    }

    Ok(trimmed.to_string())
}

fn read(config_dir: &Path) -> LoadoutFile {
    let path = loadouts_path(config_dir);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LoadoutFile::default();
        }
        Err(error) => {
            log::warn!("Could not read {}: {error}", path.display());
            return LoadoutFile::default();
        }
    };

    match serde_json::from_str::<LoadoutFile>(&raw) {
        Ok(file) => file,
        Err(error) => {
            // Keep the broken file instead of overwriting it, exactly like the
            // settings store does.
            let backup = path.with_extension(format!("json.corrupt-{}", unix_millis()));
            log::error!(
                "Loadouts file is invalid JSON ({error}); backing up to {}",
                backup.display()
            );
            if let Err(rename_error) = fs::rename(&path, &backup) {
                log::error!("Could not back up corrupt loadouts: {rename_error}");
            }
            LoadoutFile::default()
        }
    }
}

fn write(config_dir: &Path, file: &LoadoutFile) -> AppResult<()> {
    fs::create_dir_all(config_dir)?;

    let path = loadouts_path(config_dir);
    let temp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(file)?;

    {
        let mut handle = File::create(&temp)?;
        handle.write_all(json.as_bytes())?;
        handle.sync_all()?;
    }

    // On Windows, std::fs::rename replaces an existing destination atomically.
    fs::rename(&temp, &path)?;
    Ok(())
}

/// Snapshot every direct mod folder in `mods_dir`.
pub fn snapshot(mods_dir: &Path) -> AppResult<Vec<ModStateSnapshot>> {
    let entries = fs::read_dir(mods_dir)?;
    let mut states = Vec::new();

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

        let (is_disabled, base_name) = split_disabled(&folder_name);
        if base_name.trim().is_empty() {
            continue;
        }

        states.push(ModStateSnapshot {
            folder_name,
            base_name,
            enabled: !is_disabled,
        });
    }

    states.sort_by(|a, b| a.base_name.to_lowercase().cmp(&b.base_name.to_lowercase()));
    Ok(states)
}

pub fn list(config_dir: &Path) -> Vec<LoadoutSummary> {
    let mut summaries: Vec<LoadoutSummary> = read(config_dir)
        .loadouts
        .into_iter()
        .map(|loadout| LoadoutSummary {
            name: loadout.name,
            entry_count: loadout.entries.len() as u32,
            enabled_count: loadout.entries.iter().filter(|entry| entry.enabled).count() as u32,
            created_at: loadout.created_at,
            updated_at: loadout.updated_at,
        })
        .collect();

    summaries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    summaries
}

pub fn get(config_dir: &Path, name: &str) -> AppResult<Loadout> {
    let name = validate_name(name)?;

    read(config_dir)
        .loadouts
        .into_iter()
        .find(|loadout| loadout.name.eq_ignore_ascii_case(&name))
        .ok_or(AppError::LoadoutNotFound(name))
}

/// Snapshot the current mod states under `name`, replacing an existing loadout
/// with the same name. The original creation time is preserved.
pub fn save(config_dir: &Path, mods_dir: &Path, name: &str) -> AppResult<LoadoutSummary> {
    let name = validate_name(name)?;
    let states = snapshot(mods_dir)?;
    let entries: Vec<LoadoutEntry> = states
        .into_iter()
        .map(|state| LoadoutEntry {
            base_name: state.base_name,
            enabled: state.enabled,
        })
        .collect();

    let mut file = read(config_dir);
    let now = iso_utc_now();

    if let Some(existing) = file
        .loadouts
        .iter_mut()
        .find(|loadout| loadout.name.eq_ignore_ascii_case(&name))
    {
        existing.name = name.clone();
        existing.updated_at = now;
        existing.entries = entries;
    } else {
        file.loadouts.push(Loadout {
            name: name.clone(),
            created_at: now.clone(),
            updated_at: now,
            entries,
        });
    }

    write(config_dir, &file)?;

    let loadout = file
        .loadouts
        .into_iter()
        .find(|loadout| loadout.name == name)
        .ok_or_else(|| AppError::LoadoutNotFound(name))?;

    Ok(LoadoutSummary {
        name: loadout.name,
        entry_count: loadout.entries.len() as u32,
        enabled_count: loadout.entries.iter().filter(|entry| entry.enabled).count() as u32,
        created_at: loadout.created_at,
        updated_at: loadout.updated_at,
    })
}

pub fn delete(config_dir: &Path, name: &str) -> AppResult<()> {
    let name = validate_name(name)?;
    let mut file = read(config_dir);
    let before = file.loadouts.len();

    file.loadouts
        .retain(|loadout| !loadout.name.eq_ignore_ascii_case(&name));

    if file.loadouts.len() == before {
        return Err(AppError::LoadoutNotFound(name));
    }

    write(config_dir, &file)
}

/// Write a timestamped copy of the current enabled/disabled state before a
/// destructive batch operation. Returns `None` when there is nothing to back up.
pub fn write_state_backup(config_dir: &Path, mods_dir: &Path) -> AppResult<Option<PathBuf>> {
    let states = snapshot(mods_dir)?;
    if states.is_empty() {
        return Ok(None);
    }

    let backups = config_dir.join("backups");
    fs::create_dir_all(&backups)?;

    let stamp = iso_utc_now().replace(':', "-");
    let path = backups.join(format!("mod-state-{stamp}-{}.json", unix_millis()));
    let temp = path.with_extension("json.tmp");

    let payload = StateBackup {
        created_at: iso_utc_now(),
        mods_dir: mods_dir.display().to_string(),
        states,
    };
    let json = serde_json::to_string_pretty(&payload)?;

    {
        let mut handle = File::create(&temp)?;
        handle.write_all(json.as_bytes())?;
        handle.sync_all()?;
    }
    fs::rename(&temp, &path)?;

    log::info!("Wrote mod state backup to {}", path.display());
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "zzmm-loadouts-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn make_mod(root: &Path, folder: &str) {
        fs::create_dir_all(root.join(folder)).expect("mod folder");
    }

    #[test]
    fn saves_updates_lists_and_deletes_loadouts() {
        let config = temp_dir("config");
        let mods = temp_dir("mods");
        make_mod(&mods, "Alpha");
        make_mod(&mods, "DISABLED_Beta");

        let saved = save(&config, &mods, "  Daily  ").expect("save");
        assert_eq!(saved.name, "Daily");
        assert_eq!(saved.entry_count, 2);
        assert_eq!(saved.enabled_count, 1);

        // Saving again with the same name updates in place.
        make_mod(&mods, "Gamma");
        let updated = save(&config, &mods, "daily").expect("update");
        assert_eq!(updated.entry_count, 3);
        assert_eq!(list(&config).len(), 1);
        assert_eq!(list(&config)[0].name, "daily");

        let loadout = get(&config, "DAILY").expect("get");
        assert_eq!(loadout.entries.len(), 3);
        assert!(loadout
            .entries
            .iter()
            .any(|entry| entry.base_name == "Beta" && !entry.enabled));

        delete(&config, "Daily").expect("delete");
        assert!(list(&config).is_empty());
        assert!(matches!(
            delete(&config, "Daily"),
            Err(AppError::LoadoutNotFound(_))
        ));

        fs::remove_dir_all(&config).ok();
        fs::remove_dir_all(&mods).ok();
    }

    #[test]
    fn rejects_empty_names_and_backs_up_corrupt_files() {
        let config = temp_dir("corrupt");
        assert!(matches!(
            save(&config, &config, "   "),
            Err(AppError::InvalidLoadout(_))
        ));

        fs::write(loadouts_path(&config), "{not json").expect("write corrupt file");
        assert!(list(&config).is_empty());

        let backups = fs::read_dir(&config)
            .expect("read config")
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("loadouts.json.corrupt-")
            })
            .count();
        assert_eq!(backups, 1);

        fs::remove_dir_all(&config).ok();
    }

    #[test]
    fn writes_state_backups_with_the_current_prefixes() {
        let config = temp_dir("backup-config");
        let mods = temp_dir("backup-mods");
        make_mod(&mods, "Enabled Mod");
        make_mod(&mods, "DISABLED_Disabled Mod");

        let path = write_state_backup(&config, &mods)
            .expect("backup")
            .expect("some backup");
        let raw = fs::read_to_string(&path).expect("read backup");
        assert!(raw.contains("\"Enabled Mod\""));
        assert!(raw.contains("\"DISABLED_Disabled Mod\""));

        fs::remove_dir_all(&config).ok();
        fs::remove_dir_all(&mods).ok();
    }
}
