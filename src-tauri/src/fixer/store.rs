//! What the fixer remembers between runs: the ignore list.
//!
//! One small JSON file in the app config folder, next to `settings.json`, read
//! at every scan and written with the project's atomic temp-file + rename.
//! Nothing here touches a mod's INI: ignoring a mod writes this file only. The
//! per-mod undo snapshots live beside it in `fixer-undo` (`fixer::undo`).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::fixer::model::{
    FixerIgnoredEntry, FixerSnapshotEntry, FixerStateView, IGNORE_SCOPE_FILE, IGNORE_SCOPE_MOD,
};
use crate::fixer::undo;
use crate::ini_parser::text;

pub const STATE_FILE: &str = "fixer.json";

/// Bumped when the file's shape changes: 2 dropped the per-fix history that
/// accumulated one backup per run (the bounded `fixer-undo` snapshots replaced
/// it). Older files load as they are - the unknown field is ignored and the
/// ignore list is preserved.
const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixerState {
    #[serde(default = "schema_version")]
    pub version: u32,
    #[serde(default)]
    pub ignored: Vec<FixerIgnoredEntry>,
}

fn schema_version() -> u32 {
    SCHEMA_VERSION
}

impl Default for FixerState {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            ignored: Vec::new(),
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
            let aside = path.with_extension(format!("json.corrupt-{}", crate::util::unix_now()));
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

/// The UI shape: the ignore list as stored, and every mod whose last fix can
/// still be undone, newest capture first. Whether a snapshot is usable is
/// answered by reading the undo folder on disk *now*, never cached.
pub fn view(config_dir: &Path, state: &FixerState) -> FixerStateView {
    FixerStateView {
        ignored: state.ignored.clone(),
        snapshots: undo::list(config_dir)
            .into_iter()
            .map(|snapshot| FixerSnapshotEntry {
                folder: snapshot.manifest.folder.clone(),
                captured_at: snapshot.manifest.captured_at,
                files: snapshot.manifest.files.len() as u32,
                ini_paths: snapshot
                    .manifest
                    .files
                    .iter()
                    .map(|file| file.ini_path.clone())
                    .collect(),
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

    #[test]
    fn state_round_trips_and_a_corrupt_file_is_kept_aside() {
        let dir = scratch("roundtrip");
        let file = path_in(&dir);

        let mut state = FixerState::default();
        assert!(set_ignored(&mut state, ignored("S11Mod", None, "S11Mod"), true));
        assert!(set_ignored(
            &mut state,
            ignored_fix("S11Mod", "C:\\Mods\\S11Mod\\S11.ini", "hash:3c8697e8>67821d9d"),
            true
        ));
        save(&file, &state).expect("save");

        let loaded = load(&file);
        assert_eq!(loaded.ignored, state.ignored);
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

    /// A file written by the version that kept a per-fix history still has to
    /// load: the ignore list is the user's, and nothing about it may change.
    #[test]
    fn an_older_state_file_keeps_its_ignore_list() {
        let dir = scratch("older");
        let file = path_in(&dir);
        fs::write(
            &file,
            r#"{
              "version": 1,
              "ignored": [
                {
                  "folder": "S11Mod",
                  "iniPath": null,
                  "key": null,
                  "label": "S11Mod",
                  "ignoredAt": 1700000000
                }
              ],
              "history": [
                {
                  "iniPath": "C:\\Mods\\S11Mod\\S11.ini",
                  "folder": "S11Mod",
                  "character": "Soldier11",
                  "backupPath": "C:\\Mods\\S11Mod\\DISABLED_BACKUP_1_S11.txt",
                  "fixedAt": 1700000000,
                  "updated": 1,
                  "additions": 0
                }
              ]
            }"#,
        )
        .expect("write");

        let loaded = load(&file);
        assert_eq!(loaded.ignored.len(), 1);
        assert_eq!(loaded.ignored[0].label, "S11Mod");
        assert_eq!(
            IgnoreSet::from_state(&loaded).file_scope("s11mod", Path::new("C:\\Mods\\S11Mod\\a.ini")),
            Some(IGNORE_SCOPE_MOD)
        );

        // Saving drops the history instead of carrying it forward.
        save(&file, &loaded).expect("save");
        let raw = fs::read_to_string(&file).expect("read");
        assert!(!raw.contains("history"));
        assert!(raw.contains("S11Mod"));

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
    fn the_view_reports_the_snapshots_on_disk_newest_first() {
        let dir = scratch("view");
        let mut state = FixerState::default();
        set_ignored(&mut state, ignored("S11Mod", None, "S11Mod"), true);

        assert!(view(&dir, &state).snapshots.is_empty(), "nothing captured yet");

        let capture = |folder: &str, when: u64, ini: &str| {
            let mut writer = undo::Writer::begin(&dir, folder, when).expect("begin");
            writer.add(ini, b"pre-fix\n").expect("add");
            writer.commit().expect("commit");
        };
        capture("S11Mod", 100, "C:\\Mods\\S11Mod\\S11.ini");
        capture("Ellen Mod", 200, "C:\\Mods\\Ellen Mod\\body\\merged.ini");

        let shown = view(&dir, &state);
        assert_eq!(shown.ignored.len(), 1);
        assert_eq!(shown.snapshots.len(), 2);
        assert_eq!(shown.snapshots[0].folder, "Ellen Mod", "newest capture first");
        assert_eq!(shown.snapshots[0].captured_at, 200);
        assert_eq!(shown.snapshots[0].files, 1);
        assert_eq!(shown.snapshots[0].ini_paths, vec!["C:\\Mods\\Ellen Mod\\body\\merged.ini"]);
        assert_eq!(shown.snapshots[1].folder, "S11Mod");

        // Undoing consumes the snapshot, so the view stops offering it.
        undo::discard(&dir, "Ellen Mod").expect("discard");
        assert_eq!(view(&dir, &state).snapshots.len(), 1);

        fs::remove_dir_all(&dir).ok();
    }
}
