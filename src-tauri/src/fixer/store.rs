//! What the fixer remembers between runs: the ignore list and the history of
//! successful fixes.
//!
//! One small JSON file in the app config folder, next to `settings.json`, read
//! at every scan and written with the project's atomic temp-file + rename.
//! Nothing here touches a mod's INI: ignoring a mod writes this file only.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::fixer::backup;
use crate::fixer::model::{
    FixerHistoryEntry, FixerHistoryRecord, FixerIgnoredEntry, FixerStateView,
    IGNORE_SCOPE_FILE, IGNORE_SCOPE_MOD,
};

use crate::ini_parser::text;

pub const STATE_FILE: &str = "fixer.json";

const SCHEMA_VERSION: u32 = 1;

/// History entries kept. Older records are dropped from the file; their backups
/// on disk are left alone, so trimming never destroys a restore.
const MAX_HISTORY: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixerState {
    #[serde(default = "schema_version")]
    pub version: u32,
    #[serde(default)]
    pub ignored: Vec<FixerIgnoredEntry>,
    #[serde(default)]
    pub history: Vec<FixerHistoryRecord>,
}

fn schema_version() -> u32 {
    SCHEMA_VERSION
}

impl Default for FixerState {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            ignored: Vec::new(),
            history: Vec::new(),
        }
    }
}

/// `<config dir>/fixer.json`.
pub fn path_in(config_dir: &Path) -> PathBuf {
    config_dir.join(STATE_FILE)
}

/// Read the state file, falling back to empty. An unparsable file is moved
/// aside instead of overwritten, so a user's ignore list is never silently lost.
pub fn load(path: &Path) -> FixerState {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return FixerState::default();
        }
        Err(error) => {
            log::warn!("Could not read {}: {error}", path.display());
            return FixerState::default();
        }
    };

    match serde_json::from_str::<FixerState>(&raw) {
        Ok(state) => state,
        Err(error) => {
            let aside = path.with_extension(format!("json.corrupt-{}", backup::now_seconds()));
            log::error!(
                "{} is not valid JSON ({error}); moving it aside to {}",
                path.display(),
                aside.display()
            );
            if let Err(rename_error) = fs::rename(path, &aside) {
                log::error!("Could not move the unreadable fixer state aside: {rename_error}");
            }
            FixerState::default()
        }
    }
}

pub fn save(path: &Path, state: &FixerState) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(state)?;
    text::write_atomic(path, json.as_bytes())
}

/// The ignore list, indexed for scan/apply lookups. Folder names and paths are
/// compared case-insensitively with `/` and `\` folded together, the way
/// Windows treats them.
#[derive(Debug, Default, Clone)]
pub struct IgnoreSet {
    /// Mod folders ignored whole.
    mods: HashSet<String>,
    /// INI paths ignored whole.
    files: HashSet<String>,
    /// (normalized INI path, recommendation key) pairs.
    fixes: HashSet<(String, String)>,
}

impl IgnoreSet {
    pub fn from_state(state: &FixerState) -> Self {
        let mut set = Self::default();
        for entry in &state.ignored {
            match (&entry.ini_path, &entry.key) {
                (Some(ini), Some(key)) => {
                    set.fixes.insert((normalize(ini), key.clone()));
                }
                (Some(ini), None) => {
                    set.files.insert(normalize(ini));
                }
                (None, _) => {
                    set.mods.insert(normalize(&entry.folder));
                }
            }
        }
        set
    }

    /// The whole-file scopes covering this INI, most specific first.
    pub fn file_scope(&self, folder: &str, ini: &Path) -> Option<&'static str> {
        if self.files.contains(&normalize(&ini.display().to_string())) {
            return Some(IGNORE_SCOPE_FILE);
        }
        if self.mods.contains(&normalize(folder)) {
            return Some(IGNORE_SCOPE_MOD);
        }
        None
    }

    /// Recommendation keys ignored inside this INI.
    pub fn keys_for(&self, ini: &Path) -> HashSet<String> {
        let path = normalize(&ini.display().to_string());
        self.fixes
            .iter()
            .filter(|(ignored_ini, _)| *ignored_ini == path)
            .map(|(_, key)| key.clone())
            .collect()
    }
}

fn normalize(value: &str) -> String {
    value.replace('/', "\\").to_ascii_lowercase()
}

/// Add or remove one entry. Ignoring the same (folder, iniPath, key) twice
/// refreshes its label instead of duplicating it. Returns true when the list
/// changed.
pub fn set_ignored(state: &mut FixerState, entry: FixerIgnoredEntry, ignored: bool) -> bool {
    let matches = |existing: &FixerIgnoredEntry| {
        normalize(&existing.folder) == normalize(&entry.folder)
            && same_path(existing.ini_path.as_deref(), entry.ini_path.as_deref())
            && existing.key == entry.key
    };

    if ignored {
        if let Some(existing) = state.ignored.iter_mut().find(|existing| matches(existing)) {
            let changed = existing.label != entry.label;
            existing.label = entry.label;
            return changed;
        }
        state.ignored.push(entry);
        return true;
    }

    let before = state.ignored.len();
    state.ignored.retain(|existing| !matches(existing));
    state.ignored.len() != before
}

/// `None` and `Some` never match; two paths match case- and separator-insensitive.
fn same_path(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => normalize(left) == normalize(right),
        (None, None) => true,
        _ => false,
    }
}

/// Append the records of a successful run, bumping `fixedAt` by a second when
/// two fixes of one file land in the same second, so `(iniPath, fixedAt)` stays
/// a unique id for a restore request - the same trick `backup::stash` uses for
/// backup file names.
pub fn append_history(state: &mut FixerState, records: Vec<FixerHistoryRecord>) {
    for mut record in records {
        while state.history.iter().any(|existing| same_fix(existing, &record)) {
            record.fixed_at += 1;
        }
        state.history.push(record);
    }

    if state.history.len() > MAX_HISTORY {
        state.history.drain(..state.history.len() - MAX_HISTORY);
        log::info!("Fixer history trimmed to the newest {MAX_HISTORY} entries");
    }
}

fn same_fix(left: &FixerHistoryRecord, right: &FixerHistoryRecord) -> bool {
    left.fixed_at == right.fixed_at && normalize(&left.ini_path) == normalize(&right.ini_path)
}

/// The recorded fix a restore request refers to. The backup path comes from
/// here, never from the UI.
pub fn find_history<'a>(
    state: &'a FixerState,
    ini_path: &str,
    fixed_at: u64,
) -> Option<&'a FixerHistoryRecord> {
    state
        .history
        .iter()
        .find(|record| record.fixed_at == fixed_at && normalize(&record.ini_path) == normalize(ini_path))
}

/// Mark a recorded fix as undone. False when the entry is gone.
pub fn mark_restored(state: &mut FixerState, ini_path: &str, fixed_at: u64, when: u64) -> bool {
    match state.history.iter_mut().find(|record| {
        record.fixed_at == fixed_at && normalize(&record.ini_path) == normalize(ini_path)
    }) {
        Some(record) => {
            record.restored_at = Some(when);
            true
        }
        None => false,
    }
}

/// The UI shape: the ignore list as stored, and the history newest first with
/// every backup checked on disk right now.
pub fn view(state: &FixerState) -> FixerStateView {
    FixerStateView {
        ignored: state.ignored.clone(),
        history: state
            .history
            .iter()
            .rev()
            .map(|record| FixerHistoryEntry {
                ini_path: record.ini_path.clone(),
                folder: record.folder.clone(),
                character: record.character.clone(),
                backup_path: record.backup_path.clone(),
                backup_available: record
                    .backup_path
                    .as_deref()
                    .is_some_and(|path| Path::new(path).is_file()),
                fixed_at: record.fixed_at,
                updated: record.updated,
                additions: record.additions,
                restored_at: record.restored_at,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zzmm-fixer-store-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn ignored(folder: &str, ini_path: Option<&str>, label: &str) -> FixerIgnoredEntry {
        FixerIgnoredEntry {
            folder: folder.to_string(),
            ini_path: ini_path.map(str::to_string),
            key: None,
            label: label.to_string(),
            ignored_at: 1,
        }
    }

    fn ignored_fix(folder: &str, ini_path: &str, key: &str) -> FixerIgnoredEntry {
        FixerIgnoredEntry {
            folder: folder.to_string(),
            ini_path: Some(ini_path.to_string()),
            key: Some(key.to_string()),
            label: format!("{ini_path} · {key}"),
            ignored_at: 1,
        }
    }

    fn record(ini_path: &str, fixed_at: u64, backup: Option<&str>) -> FixerHistoryRecord {
        FixerHistoryRecord {
            ini_path: ini_path.to_string(),
            folder: "Mod".to_string(),
            character: "Soldier11".to_string(),
            backup_path: backup.map(str::to_string),
            fixed_at,
            updated: 2,
            additions: 1,
            restored_at: None,
        }
    }

    #[test]
    fn state_round_trips_and_a_corrupt_file_is_kept_aside() {
        let dir = scratch("roundtrip");
        let file = path_in(&dir);

        let mut state = FixerState::default();
        assert!(set_ignored(&mut state, ignored("S11Mod", None, "S11Mod"), true));
        append_history(&mut state, vec![record("C:\\Mods\\S11Mod\\S11.ini", 10, None)]);
        save(&file, &state).expect("save");

        let loaded = load(&file);
        assert_eq!(loaded.ignored, state.ignored);
        assert_eq!(loaded.history, state.history);
        assert_eq!(loaded.version, SCHEMA_VERSION);

        fs::write(&file, "{ not json").expect("corrupt it");
        assert_eq!(load(&file).ignored, Vec::new());
        let kept_aside = fs::read_dir(&dir)
            .expect("dir")
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().contains("corrupt"))
            .count();
        assert_eq!(kept_aside, 1, "the unreadable file is not deleted");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_mod_ignore_covers_every_ini_below_it_and_a_file_ignore_only_its_file() {
        let mut state = FixerState::default();
        set_ignored(&mut state, ignored("S11Mod", None, "S11Mod"), true);
        set_ignored(
            &mut state,
            ignored("Ellen Mod", Some("C:/Mods/Ellen Mod/body/merged.ini"), "Ellen Mod\\body\\merged.ini"),
            true,
        );
        set_ignored(
            &mut state,
            ignored_fix(
                "Ellen Mod",
                "C:/Mods/Ellen Mod/hair/merged.ini",
                "hash:3c8697e8>67821d9d",
            ),
            true,
        );
        let set = IgnoreSet::from_state(&state);

        // Case and separator differences do not matter; the mod scope covers a
        // file that was never named.
        assert_eq!(
            set.file_scope("s11mod", Path::new("C:\\Mods\\S11Mod\\nested\\deep.ini")),
            Some(IGNORE_SCOPE_MOD)
        );
        assert_eq!(
            set.file_scope("Ellen Mod", Path::new("C:\\Mods\\Ellen Mod\\body\\merged.ini")),
            Some(IGNORE_SCOPE_FILE)
        );
        assert_eq!(
            set.file_scope("Ellen Mod", Path::new("C:\\Mods\\Ellen Mod\\hair\\merged.ini")),
            None,
            "a per-recommendation ignore never hides the whole file"
        );
        assert_eq!(set.file_scope("Vivian", Path::new("C:\\Mods\\Vivian\\v.ini")), None);

        // Only the named recommendation of the named file is covered.
        let hair = Path::new("C:\\Mods\\Ellen Mod\\hair\\merged.ini");
        let keys = set.keys_for(hair);
        assert_eq!(keys.len(), 1);
        assert!(keys.contains("hash:3c8697e8>67821d9d"));
        assert!(set.keys_for(Path::new("C:\\Mods\\Ellen Mod\\body\\merged.ini")).is_empty());
    }

    #[test]
    fn ignoring_twice_refreshes_and_unignoring_removes() {
        let mut state = FixerState::default();

        assert!(set_ignored(&mut state, ignored("Mod", None, "Mod"), true));
        assert!(!set_ignored(&mut state, ignored("Mod", None, "Mod"), true));
        assert_eq!(state.ignored.len(), 1);

        assert!(set_ignored(&mut state, ignored("mod", None, "Mod (renamed)"), true));
        assert_eq!(state.ignored.len(), 1, "same scope is not duplicated");
        assert_eq!(state.ignored[0].label, "Mod (renamed)");

        assert!(set_ignored(&mut state, ignored("Mod", Some("C:\\Mods\\Mod\\a.ini"), "a"), true));
        assert_eq!(state.ignored.len(), 2, "a file scope is a separate entry");

        assert!(set_ignored(
            &mut state,
            ignored_fix("Mod", "C:\\Mods\\Mod\\a.ini", "hash:aa>bb"),
            true
        ));
        assert_eq!(state.ignored.len(), 3, "a recommendation scope is its own entry");

        // Unignoring one scope leaves the others alone.
        assert!(set_ignored(&mut state, ignored("MOD", None, "Mod"), false));
        assert_eq!(state.ignored.len(), 2);
        assert!(set_ignored(
            &mut state,
            ignored_fix("Mod", "c:\\mods\\mod\\A.ini", "hash:aa>bb"),
            false
        ));
        assert_eq!(state.ignored.len(), 1);
        assert!(!set_ignored(&mut state, ignored("Mod", None, "Mod"), false));
    }

    #[test]
    fn history_keeps_one_id_per_fix_and_reports_availability_from_disk() {
        let dir = scratch("history");
        let file = path_in(&dir);
        let backup = dir.join("DISABLED_BACKUP_100_S11.txt");
        fs::write(&backup, b"[TextureOverrideX]\r\nhash = 3c8697e8\r\n").expect("backup");

        let mut state = FixerState::default();
        append_history(&mut state, vec![record("C:\\Mods\\S11.ini", 100, Some(&backup.display().to_string()))]);
        // A second fix of the same file in the same second must not collide.
        append_history(&mut state, vec![record("C:\\Mods\\S11.ini", 100, None)]);
        assert_eq!(state.history[1].fixed_at, 101);

        let shown = view(&state);
        assert_eq!(shown.history.len(), 2);
        assert_eq!(shown.history[0].fixed_at, 101, "newest first");
        assert!(!shown.history[0].backup_available, "no backup was kept");
        assert!(shown.history[1].backup_available);

        assert!(find_history(&state, "c:\\mods\\s11.INI", 100).is_some());
        assert!(find_history(&state, "C:\\Mods\\S11.ini", 99).is_none());

        assert!(mark_restored(&mut state, "C:\\Mods\\S11.ini", 100, 500));
        assert_eq!(state.history[0].restored_at, Some(500));

        // A consumed backup reads as unavailable, not as "restorable".
        fs::remove_file(&backup).expect("consume");
        save(&file, &state).expect("save");
        assert!(!view(&load(&file)).history[1].backup_available);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn history_is_trimmed_to_the_newest_entries() {
        let mut state = FixerState::default();
        let records = (0..MAX_HISTORY as u64 + 5)
            .map(|index| record(&format!("C:\\Mods\\Mod{index}\\m.ini"), index, None))
            .collect();
        append_history(&mut state, records);

        assert_eq!(state.history.len(), MAX_HISTORY);
        assert_eq!(state.history[0].fixed_at, 5);
    }
}
