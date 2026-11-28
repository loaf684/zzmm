//! One restorable pre-fix snapshot per mod, under the app config folder.
//!
//! Upstream ZZZ-Mod-Fixer keeps every original it ever moved, named after the
//! second it was written (`DISABLED_BACKUP_<time>_<name>.txt`), so a mod fixed
//! after each game update leaves a pile of copies nobody can tell apart. ZZMM
//! instead keeps **at most one snapshot per mod**: `fixer_apply` captures the
//! bytes a run is about to overwrite into `<config>/fixer-undo/<mod>/`, and a
//! later run on the same mod **replaces** that folder, so undo always returns
//! the mod to how it was right before the most recent fix, and the storage
//! never grows with the number of runs.
//!
//! A snapshot folder is self-describing (`manifest.json` plus one `.bin` per
//! file) and is written as a whole:
//!
//! 1. bytes and manifest go into a hidden temp folder next to the final one;
//! 2. the previous snapshot folder is removed;
//! 3. the temp folder is renamed onto the final name.
//!
//! The rename is the atomic step, so a crash or a full disk leaves either the
//! old snapshot or none - never a half-written one. Stored files are indexed
//! (`000.bin`, `001.bin`, ...) and named in the manifest, so two INIs with the
//! same file name in one mod (upstream's known limitation) stay distinguishable.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::ini_parser::text;
use crate::util::unix_now;

/// Folder under the app config dir holding every mod's snapshot.
pub const UNDO_DIR: &str = "fixer-undo";

/// Manifest inside one snapshot folder.
const MANIFEST: &str = "manifest.json";

const SNAPSHOT_VERSION: u32 = 1;

/// Longest human-readable part of a snapshot folder name.
const NAME_LIMIT: usize = 40;

/// Distinguishes temp folders of two runs that start in the same second.
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotManifest {
    #[serde(default = "snapshot_version")]
    pub version: u32,
    /// Mod folder: the first path component under the Mods folder.
    pub folder: String,
    /// Unix seconds the originals were read.
    pub captured_at: u64,
    /// In capture order; `stored` is the file inside this snapshot folder.
    pub files: Vec<SnapshotFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotFile {
    /// Absolute INI path at capture time.
    pub ini_path: String,
    /// File inside the snapshot folder holding the original bytes.
    pub stored: String,
}

/// A snapshot found on disk, with the folder it lives in.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub manifest: SnapshotManifest,
    pub dir: PathBuf,
}

impl Snapshot {
    /// Absolute path of the stored copy of one file.
    pub fn stored_path(&self, file: &SnapshotFile) -> PathBuf {
        self.dir.join(&file.stored)
    }
}

fn snapshot_version() -> u32 {
    SNAPSHOT_VERSION
}

/// `<config dir>/fixer-undo`.
pub fn dir_in(config_dir: &Path) -> PathBuf {
    config_dir.join(UNDO_DIR)
}

/// Windows paths: case and separators do not tell two mods apart.
fn normalize(folder: &str) -> String {
    folder.replace('/', "\\").to_ascii_lowercase()
}

/// FNV-1a, so a folder name maps to the same snapshot folder on every run and
/// after every app update (unlike `std`'s hashers, whose algorithm is free to
/// change). Two mods with the same readable prefix still differ in the suffix.
fn fingerprint(value: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Folder name of one mod's snapshot: a readable, filesystem-safe prefix plus a
/// stable fingerprint of the normalized folder, which makes the mapping a
/// one-to-one with no collisions in practice and no index to keep in sync.
fn key(folder: &str) -> String {
    let mut readable: String = folder
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '.' | '_') {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .take(NAME_LIMIT)
        .collect();
    if readable.is_empty() {
        readable.push_str("mod");
    }
    format!("{readable}-{:016x}", fingerprint(&normalize(folder)))
}

fn snapshot_dir(config_dir: &Path, folder: &str) -> PathBuf {
    dir_in(config_dir).join(key(folder))
}

fn read_manifest(dir: &Path) -> Option<SnapshotManifest> {
    let raw = fs::read_to_string(dir.join(MANIFEST)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Builds one mod's snapshot, one file at a time, and replaces the previous one
/// on [`Writer::commit`]. Dropping it without committing removes everything it
/// wrote, so a failure never leaves a partial snapshot behind.
pub struct Writer {
    tmp: PathBuf,
    final_dir: PathBuf,
    manifest: SnapshotManifest,
    committed: bool,
}

impl Writer {
    pub fn begin(config_dir: &Path, folder: &str, captured_at: u64) -> AppResult<Self> {
        let parent = dir_in(config_dir);
        fs::create_dir_all(&parent)?;

        let tmp = parent.join(format!(
            ".tmp-{}-{}-{}",
            std::process::id(),
            unix_now(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp)?;

        Ok(Self {
            tmp,
            final_dir: snapshot_dir(config_dir, folder),
            manifest: SnapshotManifest {
                version: SNAPSHOT_VERSION,
                folder: folder.to_string(),
                captured_at,
                files: Vec::new(),
            },
            committed: false,
        })
    }

    /// Record one file's pre-fix bytes. The caller passes the bytes it read
    /// before rewriting, so they are exactly what an undo must put back.
    pub fn add(&mut self, ini_path: &str, bytes: &[u8]) -> AppResult<()> {
        let stored = format!("{:03}.bin", self.manifest.files.len());
        fs::write(self.tmp.join(&stored), bytes)?;
        self.manifest.files.push(SnapshotFile {
            ini_path: ini_path.to_string(),
            stored,
        });
        Ok(())
    }

    /// Replace this mod's snapshot with what was recorded. The rename is what
    /// makes the swap atomic; a snapshot with no files is discarded instead.
    pub fn commit(mut self) -> AppResult<()> {
        if self.manifest.files.is_empty() {
            return Ok(());
        }

        let json = serde_json::to_vec_pretty(&self.manifest)?;
        fs::write(self.tmp.join(MANIFEST), &json)?;

        if self.final_dir.exists() {
            fs::remove_dir_all(&self.final_dir)?;
        }
        fs::rename(&self.tmp, &self.final_dir)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.tmp);
        }
    }
}

/// The snapshot stored for one mod, if there is one.
pub fn load(config_dir: &Path, folder: &str) -> Option<Snapshot> {
    let dir = snapshot_dir(config_dir, folder);
    let manifest = read_manifest(&dir)?;
    Some(Snapshot { manifest, dir })
}

/// Every snapshot on disk, newest first. Folders without a readable manifest
/// (a leftover temp folder, a hand-broken snapshot) are ignored rather than
/// reported as restorable.
pub fn list(config_dir: &Path) -> Vec<Snapshot> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir_in(config_dir)) else {
        return found;
    };

    for entry in entries.flatten() {
        if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }
        let dir = entry.path();
        if let Some(manifest) = read_manifest(&dir) {
            found.push(Snapshot { manifest, dir });
        }
    }

    found.sort_by(|left, right| right.manifest.captured_at.cmp(&left.manifest.captured_at));
    found
}

/// Drop one mod's snapshot. Removing what is not there is not an error.
pub fn discard(config_dir: &Path, folder: &str) -> AppResult<()> {
    match fs::remove_dir_all(snapshot_dir(config_dir, folder)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Keep only `kept` in one mod's snapshot, so a partial undo can be retried
/// without restoring (or claiming) the files that already came back. The stored
/// file names are unchanged, so the remaining entries stay valid.
pub fn retain(config_dir: &Path, folder: &str, kept: &[SnapshotFile]) -> AppResult<()> {
    let Some(mut snapshot) = load(config_dir, folder) else {
        return Ok(());
    };

    snapshot.manifest.files = kept.to_vec();
    let json = serde_json::to_vec_pretty(&snapshot.manifest)?;
    text::write_atomic(&snapshot.dir.join(MANIFEST), &json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zzmm-fixer-undo-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn capture(config_dir: &Path, folder: &str, when: u64, files: &[(&str, &[u8])]) {
        let mut writer = Writer::begin(config_dir, folder, when).expect("begin");
        for (ini_path, bytes) in files {
            writer.add(ini_path, bytes).expect("add");
        }
        writer.commit().expect("commit");
    }

    #[test]
    fn capture_replaces_the_previous_snapshot_of_the_same_mod() {
        let dir = scratch("bounded");

        capture(&dir, "S11Mod", 100, &[("C:\\Mods\\S11Mod\\S11.ini", b"one\n")]);
        let first = load(&dir, "S11Mod").expect("first snapshot");
        assert_eq!(first.manifest.captured_at, 100);
        assert_eq!(fs::read(first.stored_path(&first.manifest.files[0])).expect("read"), b"one\n");

        // The second run touched a different file of the same mod; the snapshot
        // becomes *that* run's pre-fix state and the earlier copy is gone.
        capture(&dir, "S11Mod", 200, &[("C:\\Mods\\S11Mod\\Other.ini", b"two\n")]);
        assert_eq!(list(&dir).len(), 1, "one snapshot per mod, however often it is fixed");

        let second = load(&dir, "S11Mod").expect("second snapshot");
        assert_eq!(second.manifest.captured_at, 200);
        assert_eq!(second.manifest.files.len(), 1);
        assert_eq!(second.manifest.files[0].ini_path, "C:\\Mods\\S11Mod\\Other.ini");
        assert_eq!(fs::read(second.stored_path(&second.manifest.files[0])).expect("read"), b"two\n");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn same_named_inis_in_one_mod_stay_distinguishable() {
        let dir = scratch("collision");
        capture(
            &dir,
            "Ellen Mod",
            10,
            &[
                ("C:\\Mods\\Ellen Mod\\body\\merged.ini", b"body\n"),
                ("C:\\Mods\\Ellen Mod\\hair\\merged.ini", b"hair\n"),
            ],
        );

        let snapshot = load(&dir, "Ellen Mod").expect("snapshot");
        assert_eq!(snapshot.manifest.files.len(), 2);
        assert_ne!(snapshot.manifest.files[0].stored, snapshot.manifest.files[1].stored);
        assert_eq!(fs::read(snapshot.stored_path(&snapshot.manifest.files[0])).expect("read"), b"body\n");
        assert_eq!(fs::read(snapshot.stored_path(&snapshot.manifest.files[1])).expect("read"), b"hair\n");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_same_mod_always_maps_to_the_same_snapshot_folder() {
        assert_eq!(key("S11Mod"), key("s11mod"));
        assert_eq!(key("Mods\\Ellen"), key("mods/ellen"));
        assert_ne!(key("Mod Name"), key("Mod_Name"));

        // Readable, but never able to escape the undo folder or hit a Windows
        // device name (a trailing suffix always follows the prefix).
        let weird = key("..\\../../AUX");
        assert!(!weird.contains('/') && !weird.contains('\\') && !weird.contains(':'));
        assert!(!weird.ends_with('.'));

        let dir = scratch("keys");
        assert_eq!(snapshot_dir(&dir, "S11Mod").parent(), Some(dir_in(&dir).as_path()));
        assert_eq!(snapshot_dir(&dir, "s11mod"), snapshot_dir(&dir, "S11Mod"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unreadable_snapshots_are_not_offered_and_can_be_discarded() {
        let dir = scratch("broken");
        capture(&dir, "Good", 1, &[("C:\\Mods\\Good\\g.ini", b"ok\n")]);

        let broken = dir_in(&dir).join("broken-0000");
        fs::create_dir_all(&broken).expect("dir");
        fs::write(broken.join(MANIFEST), "{ not json").expect("manifest");

        let found = list(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.folder, "Good");
        assert!(load(&dir, "Missing").is_none());

        discard(&dir, "Good").expect("discard");
        assert!(list(&dir).is_empty());
        discard(&dir, "Good").expect("discarding twice is fine");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn retain_keeps_only_the_files_an_undo_still_owes() {
        let dir = scratch("retain");
        capture(
            &dir,
            "Mod",
            5,
            &[("C:\\Mods\\Mod\\a.ini", b"a\n"), ("C:\\Mods\\Mod\\b.ini", b"b\n")],
        );

        let snapshot = load(&dir, "Mod").expect("snapshot");
        let remaining = vec![snapshot.manifest.files[1].clone()];
        retain(&dir, "Mod", &remaining).expect("retain");

        let kept = load(&dir, "Mod").expect("snapshot");
        assert_eq!(kept.manifest.files, remaining);
        assert_eq!(kept.manifest.captured_at, 5, "the capture stays honest");
        assert_eq!(fs::read(kept.stored_path(&kept.manifest.files[0])).expect("read"), b"b\n");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_writer_that_is_dropped_early_leaves_nothing_behind() {
        let dir = scratch("abandon");
        capture(&dir, "Mod", 1, &[("C:\\Mods\\Mod\\a.ini", b"old\n")]);

        let mut writer = Writer::begin(&dir, "Mod", 2).expect("begin");
        writer.add("C:\\Mods\\Mod\\a.ini", b"new\n").expect("add");
        drop(writer);

        // The earlier snapshot is untouched and no temp folder survives.
        let snapshot = load(&dir, "Mod").expect("snapshot");
        assert_eq!(snapshot.manifest.captured_at, 1);
        let entries: Vec<_> = fs::read_dir(dir_in(&dir))
            .expect("dir")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(entries, vec![key("Mod")], "no temp folder is left behind");

        fs::remove_dir_all(&dir).ok();
    }
}
