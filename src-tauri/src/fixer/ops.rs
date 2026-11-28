//! File-system side of the fixer: walking a Mods folder, reading INIs in their
//! own encoding, writing them back through a backup, and rolling back.
//!
//! Everything here is plain functions over paths so it can be unit tested; the
//! Tauri command wrappers live in `commands.rs`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::fixer::backup;
use crate::fixer::database::Database;
use crate::fixer::engine;
use crate::fixer::model::{
    FixerApplied, FixerApplyResult, FixerBufferPending, FixerFailure, FixerFileReport,
    FixerHistoryRecord, FixerRollbackResult, FixerScanResult, IGNORE_SCOPE_FIX,
};
use crate::fixer::store::IgnoreSet;
use crate::ini_parser::text::{self, Encoding};

/// Refuse to load absurd files rather than exhaust memory on a stray dump.
const MAX_INI_BYTES: u64 = 32 * 1024 * 1024;

/// Folder nesting the walk descends into. Mods are two or three levels deep.
const MAX_DEPTH: usize = 12;

// MARK: encoding

/// Encoding an INI is read and written with. `Gb2312` is upstream's fallback for
/// Chinese mod INIs; `Latin1` is the byte-preserving last resort (upstream gives
/// up on such files).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileEncoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Gb2312,
    Latin1,
}


/// UTF-8, then gb2312 (only when it decodes *and* re-encodes byte-identically),
/// then the INI parser's byte-preserving single-byte fallback.
pub fn decode(bytes: &[u8]) -> (String, FileEncoding) {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return (text::decode(bytes).0, FileEncoding::Utf16Le);
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return (text::decode(bytes).0, FileEncoding::Utf8Bom);
    }
    if let Ok(decoded) = std::str::from_utf8(bytes) {
        return (decoded.to_string(), FileEncoding::Utf8);
    }

    let (decoded, had_errors) = encoding_rs::GBK.decode_without_bom_handling(bytes);
    if !had_errors {
        let candidate = decoded.into_owned();
        let (encoded, _, encode_errors) = encoding_rs::GBK.encode(&candidate);
        if !encode_errors && encoded.as_ref() == bytes {
            return (candidate, FileEncoding::Gb2312);
        }
    }

    (text::decode(bytes).0, FileEncoding::Latin1)
}

pub fn encode(content: &str, encoding: FileEncoding) -> Vec<u8> {
    match encoding {
        FileEncoding::Utf8 => content.as_bytes().to_vec(),
        FileEncoding::Gb2312 => {
            let (encoded, _, errors) = encoding_rs::GBK.encode(content);
            debug_assert!(!errors, "gb2312 content that cannot be re-encoded");
            encoded.into_owned()
        }
        other => text::encode(content, parser_encoding(other).expect("bom/utf-16/latin-1")),
    }
}

fn parser_encoding(encoding: FileEncoding) -> Option<Encoding> {
    match encoding {
        FileEncoding::Utf8 | FileEncoding::Gb2312 => None,
        FileEncoding::Utf8Bom => Some(Encoding::Utf8Bom),
        FileEncoding::Utf16Le => Some(Encoding::Utf16Le),
        FileEncoding::Latin1 => Some(Encoding::Latin1),
    }
}

// MARK: scan

/// Read-only scan. Never writes anything, not even a backup.
pub fn scan(mods_dir: &Path, ignored: &IgnoreSet) -> AppResult<FixerScanResult> {
    if !mods_dir.is_dir() {
        return Err(AppError::ModsDirMissing(mods_dir.display().to_string()));
    }

    // Walk the path as given: canonicalising it would put a `\\?\` verbatim
    // prefix on every `iniPath` the UI shows.
    let mut state = Walk {
        root: mods_dir.to_path_buf(),
        ignored,
        scanned: 0,
        skipped: 0,
        files: Vec::new(),
    };
    state.walk(mods_dir, 0);

    Ok(FixerScanResult {
        scanned: state.scanned,
        skipped: state.skipped,
        files: state.files,
    })
}

struct Walk<'a> {
    root: PathBuf,
    ignored: &'a IgnoreSet,
    scanned: u32,
    skipped: u32,
    files: Vec<FixerFileReport>,
}

impl Walk {
    fn walk(&mut self, dir: &Path, depth: usize) {
        let Ok(entries) = fs::read_dir(dir) else {
            log::warn!("Could not read {}", dir.display());
            return;
        };

        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());

        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let upper = name.to_ascii_uppercase();
            let path = entry.path();
            let file_type = entry.file_type();

            // ZZMI ignores anything named DESKTOP*, files and folders alike.
            if upper.starts_with("DESKTOP") {
                if is_ini(&name) && file_type.map(|t| t.is_file()).unwrap_or(false) {
                    self.skipped += 1;
                }
                continue;
            }

            match file_type {
                Ok(kind) if kind.is_dir() => {
                    if depth + 1 < MAX_DEPTH {
                        self.walk(&path, depth + 1);
                    } else {
                        log::warn!("Not descending past {} levels into {}", MAX_DEPTH, path.display());
                    }
                }
                Ok(kind) if kind.is_file() && is_ini(&name) => {
                    if upper.starts_with("DISABLED") {
                        self.skipped += 1;
                        continue;
                    }
                    self.scanned += 1;
                    if let Some(report) = inspect(&path, &self.root, self.ignored) {
                        self.files.push(report);
                    }
                }
                // Symlinks are skipped: a loop in a mod folder must not walk us
                // around in circles.
                _ => {}
            }
        }
    }
}

fn is_ini(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("ini"))
}

/// The mod folder an INI belongs to: its first path component under the Mods
/// folder, or its own name for a file sitting in the Mods folder itself.
fn folder_of(root: &Path, ini: &Path) -> String {
    let relative = ini.strip_prefix(root).unwrap_or(ini);
    let mut components = relative.components();

    match (components.next(), components.next()) {
        (Some(first), Some(_)) => first.as_os_str().to_string_lossy().to_string(),
        _ => relative
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
    }
}

/// Run the engine over one INI. `None` when the file is already up to date.
fn inspect(ini: &Path, root: &Path, ignored: &IgnoreSet) -> Option<FixerFileReport> {
    let folder = folder_of(root, ini);
    let ini_path = ini.display().to_string();
    let scope = ignored.file_scope(&folder, ini);

    let content = match fs::metadata(ini) {
        Ok(metadata) if metadata.len() > MAX_INI_BYTES => {
            log::warn!("Skipping oversized INI {}", ini.display());
            return None;
        }
        Ok(_) => match fs::read(ini) {
            Ok(bytes) => decode(&bytes).0,
            Err(error) => {
                return Some(broken(folder, ini_path, format!("could not read: {error}"), scope));
            }
        },
        Err(error) => {
            return Some(broken(folder, ini_path, format!("could not stat: {error}"), scope));
        }
    };

    // A file- or mod-scoped ignore does not hide *what* would be fixed: the
    // engine runs unfiltered and every row is marked ignored. A
    // recommendation-scoped ignore filters the engine, which then reports the
    // ignored rewrite without performing it - and without the fixes that keyed
    // off it, exactly as a later apply with the same list behaves.
    let keys = match scope {
        Some(_) => HashSet::new(),
        None => ignored.keys_for(ini),
    };
    let outcome = engine::run_with_ignore(&content, Database::bundled(), &keys);
    let mut report = outcome.report;

    if let Some(error) = report.error {
        return Some(broken(folder, ini_path, error, scope));
    }

    if report.updated.is_empty() && report.additions.is_empty() && report.unsupported.is_empty() {
        return None;
    }

    let mut ignored_scope = scope;
    if scope.is_some() {
        for update in &mut report.updated {
            update.ignored = true;
        }
        for addition in &mut report.additions {
            addition.ignored = true;
        }
    } else if !report.updated.is_empty() || !report.additions.is_empty() {
        // Only the per-recommendation list applied: the file is untouched when
        // every recommendation it has was ignored.
        if report.updated.iter().all(|change| change.ignored)
            && report.additions.iter().all(|addition| addition.ignored)
        {
            ignored_scope = Some(IGNORE_SCOPE_FIX);
        }
    }

    let character = report.character();
    Some(FixerFileReport {
        folder,
        ini_path,
        character,
        characters: report.characters,
        updated: report.updated,
        additions: report.additions,
        unsupported: report.unsupported,
        needs_buffer_pass: report.needs_buffer_pass,
        ignored: ignored_scope.is_some(),
        ignored_scope,
        error: None,
    })
}

fn broken(
    folder: String,
    ini_path: String,
    error: String,
    scope: Option<&'static str>,
) -> FixerFileReport {
    FixerFileReport {
        folder,
        ini_path,
        character: "Unknown".to_string(),
        characters: Vec::new(),
        updated: Vec::new(),
        additions: Vec::new(),
        unsupported: Vec::new(),
        needs_buffer_pass: false,
        ignored: scope.is_some(),
        ignored_scope: scope,
        error: Some(error),
    }
}

// MARK: apply

/// Rewrite the given INIs. With `backup` the original is moved to the backup
/// folder first, exactly where upstream puts it.
///
/// Files covered by `ignored` are never read and never written: the ignore list
/// is a promise, not a UI hint, so it is enforced here as well.
pub fn apply(
    mods_dir: &Path,
    ini_paths: &[String],
    backup: bool,
    ignored: &IgnoreSet,
) -> AppResult<FixerApplyResult> {
    let root = canonical_dir(mods_dir)?;
    let mut result = FixerApplyResult {
        applied: Vec::new(),
        failed: Vec::new(),
        needs_buffer_pass: Vec::new(),
        unchanged: Vec::new(),
        ignored: Vec::new(),
    };
    let timestamp = backup::now_seconds();

    for path in ini_paths {
        let ini = PathBuf::from(path);

        if let Err(error) = ensure_inside(&root, &ini) {
            result.failed.push(FixerFailure {
                ini_path: path.clone(),
                error,
            });
            continue;
        }

        // Same walk roots the scan used, so the folder matches the ignore list
        // and the history entry no matter how the Mods path is spelled.
        let folder = folder_of(mods_dir, &ini);
        if let Some(scope) = ignored.file_scope(&folder, &ini) {
            log::info!("Fixer apply: {} left alone ({scope} ignored)", ini.display());
            result.ignored.push(path.clone());
            continue;
        }

        let bytes = match fs::read(&ini) {
            Ok(bytes) => bytes,
            Err(error) => {
                result.failed.push(FixerFailure {
                    ini_path: path.clone(),
                    error: format!("could not read: {error}"),
                });
                continue;
            }
        };

        let (content, encoding) = decode(&bytes);
        let keys = ignored.keys_for(&ini);
        let outcome = engine::run_with_ignore(&content, Database::bundled(), &keys);
        let report = outcome.report;

        if let Some(error) = report.error {
            result.failed.push(FixerFailure {
                ini_path: path.clone(),
                error,
            });
            continue;
        }

        if !report.touched {
            let pending: Vec<_> = report
                .unsupported
                .iter()
                .filter(|entry| entry.reason == engine::REASON_BUFFER)
                .cloned()
                .collect();

            let skipped = report.updated.iter().any(|change| change.ignored)
                || report.additions.iter().any(|addition| addition.ignored);
            if skipped {
                result.ignored.push(path.clone());
            }

            if !pending.is_empty() {
                result.needs_buffer_pass.push(FixerBufferPending {
                    ini_path: path.clone(),
                    commands: pending,
                });
            } else if !skipped {
                result.unchanged.push(path.clone());
            }
            continue;
        }

        let mut backup_path = None;
        if backup {
            match backup::stash(&ini, timestamp) {
                Ok(stored) => backup_path = Some(stored.display().to_string()),
                Err(error) => {
                    result.failed.push(FixerFailure {
                        ini_path: path.clone(),
                        error: format!("could not create a backup, nothing was written: {error}"),
                    });
                    continue;
                }
            }
        }

        if let Err(error) = text::write_atomic(&ini, &encode(&outcome.text, encoding)) {
            let hint = match &backup_path {
                Some(stored) => format!("; the original is in {stored}"),
                None => String::new(),
            };
            result.failed.push(FixerFailure {
                ini_path: path.clone(),
                error: format!("could not write: {error}{hint}"),
            });
            continue;
        }

        let character = report.character();
        result.applied.push(FixerApplied {
            ini_path: path.clone(),
            folder,
            character,
            fixed_at: timestamp,
            backup_path,
            // Only what was written: an ignored recommendation is reported by
            // the scan, never claimed as applied here.
            updated: report
                .updated
                .into_iter()
                .filter(|change| !change.ignored)
                .collect(),
            additions: report
                .additions
                .into_iter()
                .filter(|addition| !addition.ignored)
                .collect(),
            unsupported: report.unsupported,
            needs_buffer_pass: report.needs_buffer_pass,
        });
    }

    Ok(result)
}

// MARK: rollback

/// Put backups back over the INIs they came from.
pub fn rollback(mods_dir: &Path, ini_paths: &[String]) -> AppResult<FixerRollbackResult> {
    let root = canonical_dir(mods_dir)?;
    let mut result = FixerRollbackResult {
        restored: Vec::new(),
        failed: Vec::new(),
    };

    for path in ini_paths {
        let ini = PathBuf::from(path);

        if let Err(error) = ensure_inside(&root, &ini) {
            result.failed.push(FixerFailure {
                ini_path: path.clone(),
                error,
            });
            continue;
        }

        match backup::restore(&ini) {
            Ok(restored_from) => {
                log::info!(
                    "Fixer rollback: {} restored from {}",
                    ini.display(),
                    restored_from.display()
                );
                result.restored.push(path.clone());
            }
            Err(error) => result.failed.push(FixerFailure {
                ini_path: path.clone(),
                error: error.to_string(),
            }),
        }
    }

    Ok(result)
}

// MARK: history restore

/// Put the original INI back from the backup one recorded fix wrote.
///
/// The backup path comes from the fixer history, never from the UI, and only
/// paths inside the Mods folder are accepted. The exact backup is consumed, so
/// an older fix of a file that was fixed twice can still be undone.
pub fn restore_history(mods_dir: &Path, record: &FixerHistoryRecord) -> Result<PathBuf, String> {
    let root = canonical_dir(mods_dir).map_err(|error| error.to_string())?;
    let ini = PathBuf::from(&record.ini_path);
    ensure_inside(&root, &ini)?;

    if record.restored_at.is_some() {
        return Err("this fix was already restored".to_string());
    }

    let Some(backup_path) = record.backup_path.as_deref() else {
        return Err("no backup was kept for this fix, so it cannot be restored".to_string());
    };

    backup::restore_from(&ini, Path::new(backup_path)).map_err(|error| error.to_string())
}

fn canonical_dir(dir: &Path) -> AppResult<PathBuf> {
    let canonical = dir
        .canonicalize()
        .map_err(|_| AppError::ModsDirMissing(dir.display().to_string()))?;

    if !canonical.is_dir() {
        return Err(AppError::ModsDirNotDir(canonical.display().to_string()));
    }

    Ok(canonical)
}

/// Refuse to touch anything outside the Mods folder. The UI can only pass paths
/// it got from a scan, but this is the boundary that protects a typo or a stale
/// path from rewriting an unrelated file.
fn ensure_inside(root: &Path, ini: &Path) -> Result<(), String> {
    let canonical = ini
        .canonicalize()
        .map_err(|_| format!("{} does not exist", ini.display()))?;

    if !canonical.starts_with(root) {
        return Err(format!(
            "{} is outside the Mods folder",
            ini.display()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zzmm-fixer-ops-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    /// A Soldier11 head-diffuse section still keyed to the 1.0 hash.
    const BROKEN: &str = "[TextureOverrideSoldier11Head]\r\nhash = 3c8697e8\r\nmatch_priority = 0\r\n\r\n[TextureOverrideOther]\r\nhash = ffffffff\r\nib = data.buf\r\n";

    fn broken_mod(root: &Path) -> PathBuf {
        let dir = root.join("S11Mod");
        fs::create_dir_all(&dir).expect("mod dir");
        let ini = dir.join("S11.ini");
        fs::write(&ini, BROKEN).expect("ini");
        ini
    }

    #[test]
    fn gb2312_files_round_trip_untouched_bytes() {
        // Chinese comments, as shipped by CN mod authors; `gb2312` is not valid
        // UTF-8, so this exercises the fallback both ways.
        let (bytes, _, _) = encoding_rs::GBK
            .encode("[TextureOverrideX]\r\n; 中文注释 hash = 3c8697e8\r\nhash = 3c8697e8\r\n");

        let (content, encoding) = decode(&bytes);
        assert_eq!(encoding, FileEncoding::Gb2312);
        assert!(content.contains("中文注释"));
        assert_eq!(encode(&content, encoding), bytes.as_ref());

        // And with one hash rewritten, the Chinese bytes survive.
        let outcome = engine::run(&content, Database::bundled());
        assert!(outcome.report.touched);
        let rewritten = encode(&outcome.text, encoding);
        let (reread, encoding) = decode(&rewritten);
        assert_eq!(encoding, FileEncoding::Gb2312);
        assert!(reread.contains("中文注释 hash = 3c8697e8"));
        assert!(reread.contains("hash = 67821d9d"));
    }

    #[test]
    fn decode_prefers_utf8_then_keeps_other_encodings_byte_exact() {
        let utf8 = "[Key]\r\nname = Éllen\r\n".as_bytes();
        assert_eq!(
            decode(utf8),
            (String::from_utf8_lossy(utf8).to_string(), FileEncoding::Utf8)
        );

        let utf16 = [0xFFu8, 0xFE, b'A', 0x00];
        let (content, encoding) = decode(&utf16);
        assert_eq!((content.as_str(), encoding), ("A", FileEncoding::Utf16Le));
        assert_eq!(encode(&content, encoding), utf16.to_vec());

        // Invalid in utf-8 *and* in gb2312: the byte-preserving fallback keeps
        // the file as it is, where upstream gives up on it.
        let odd = [0x81u8, 0xFF, b'a'];
        let (content, encoding) = decode(&odd);
        assert_eq!(encoding, FileEncoding::Latin1);
        assert_eq!(encode(&content, encoding), odd.to_vec());
    }

    #[test]
    fn skip_rules_ignore_disabled_and_desktop_inis() {
        let root = scratch("skip");
        let ini = broken_mod(&root);
        fs::write(ini.with_file_name("DISABLED_S11.ini"), BROKEN).expect("disabled");
        fs::create_dir_all(root.join("DESKTOP.ini")).expect("desktop dir");
        fs::write(root.join("DESKTOP.ini").join("nested.ini"), BROKEN).expect("desktop ini");
        // Explorer's own `desktop.ini` shares the prefix; some sandboxes refuse
        // to create a file with exactly that name, so the fixture spells it out.
        fs::write(root.join("DESKTOP_mods.ini"), BROKEN).expect("desktop-named ini");

        let result = scan(&root, &IgnoreSet::default()).expect("scan");

        assert_eq!(result.scanned, 1);
        assert_eq!(result.skipped, 2, "DISABLED_*.ini and the DESKTOP-named ini");
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].folder, "S11Mod");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn scan_reports_updates_additions_and_character_without_writing() {
        let root = scratch("scan");
        let ini = broken_mod(&root);
        let before = fs::read(&ini).expect("read");

        let result = scan(&root, &IgnoreSet::default()).expect("scan");

        assert_eq!(result.scanned, 1);
        assert_eq!(result.skipped, 0);
        let file = &result.files[0];
        assert_eq!(file.character, "Soldier11");
        assert_eq!(file.updated.len(), 1);
        assert_eq!(file.updated[0].from, "3c8697e8");
        assert_eq!(file.updated[0].to, "67821d9d");
        assert!(
            file.additions.iter().any(|added| added.section == "[TextureOverrideSoldier11.Head.IB]"),
            "the missing IB section is reported: {:?}",
            file.additions
        );
        assert!(!file.needs_buffer_pass);
        assert_eq!(fs::read(&ini).expect("read"), before, "a scan writes nothing");
        assert!(!root.join("S11Mod").join("(.ini) Caches").exists());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn apply_then_rollback_restores_the_file_byte_for_byte() {
        let root = scratch("apply");
        let ini = broken_mod(&root);
        let before = fs::read(&ini).expect("read");

        let applied = apply(&root, &[ini.display().to_string()], true, &IgnoreSet::default()).expect("apply");

        assert!(applied.failed.is_empty(), "{:?}", applied.failed);
        assert_eq!(applied.applied.len(), 1);
        let entry = &applied.applied[0];
        let backup = entry.backup_path.clone().expect("backup path");
        assert!(Path::new(&backup).is_file());

        let after = fs::read_to_string(&ini).expect("read");
        assert!(after.contains("hash = 67821d9d"));
        assert!(after.contains("; hash = 3c8697e8"), "the old value is kept as a comment");
        assert!(after.contains("[TextureOverrideSoldier11.Head.IB]"));
        assert!(after.contains("run = CommandListSkinTexture"));
        assert_all_crlf(&after);

        // A second apply finds nothing to do.
        let again = apply(&root, &[ini.display().to_string()], true, &IgnoreSet::default()).expect("apply again");
        assert!(again.applied.is_empty());
        assert_eq!(again.unchanged, vec![ini.display().to_string()]);
        assert_eq!(fs::read_to_string(&ini).expect("read"), after);

        let rolled_back = rollback(&root, &[ini.display().to_string()]).expect("rollback");
        assert!(rolled_back.failed.is_empty(), "{:?}", rolled_back.failed);
        // The IPC shape the Mod Fixer screen reads: the paths it sent back.
        assert_eq!(rolled_back.restored, vec![ini.display().to_string()]);
        assert_eq!(fs::read(&ini).expect("read"), before);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn apply_refuses_paths_outside_the_mods_folder() {
        let root = scratch("outside");
        broken_mod(&root);
        let outside = root.parent().expect("parent").join("zzmm-fixer-not-a-mod.ini");
        fs::write(&outside, BROKEN).expect("outside ini");

        let result = apply(&root, &[outside.display().to_string()], true, &IgnoreSet::default()).expect("apply");

        assert!(result.applied.is_empty());
        assert_eq!(result.failed.len(), 1);
        assert!(result.failed[0].error.contains("outside the Mods folder"));
        assert_eq!(fs::read_to_string(&outside).expect("read"), BROKEN);

        fs::remove_file(&outside).ok();
        fs::remove_dir_all(&root).ok();
    }

    /// Every newline in the rewritten file is a CRLF, including the lines the
    /// fix added; upstream emits bare `\n` for those.
    fn assert_all_crlf(text: &str) {
        for (index, _) in text.match_indices('\n') {
            assert!(
                index > 0 && text.as_bytes()[index - 1] == b'\r',
                "bare LF at byte {index} of:\n{text}"
            );
        }
    }

    /// Ignoring is enforced by `ops`, not only by the UI: an ignored mod or file
    /// is never opened, and an ignored recommendation is skipped while the rest
    /// of the file is still fixed.
    #[test]
    fn ignored_scopes_are_reported_and_never_written() {
        use crate::fixer::model::FixerIgnoredEntry;
        use crate::fixer::store;

        let root = scratch("ignore");
        let ini = broken_mod(&root);
        let before = fs::read(&ini).expect("read");
        let ini_path = ini.display().to_string();

        let entry = |key: Option<&str>| FixerIgnoredEntry {
            folder: "S11Mod".to_string(),
            ini_path: Some(ini_path.clone()),
            key: key.map(str::to_string),
            label: "S11Noisy".to_string(),
            ignored_at: 1,
        };
        let ignore_state = |key: Option<&str>| {
            let mut state = store::FixerState::default();
            store::set_ignored(&mut state, entry(key), true);
            IgnoreSet::from_state(&state)
        };

        // One recommendation ignored: the update is skipped, so the scan shows
        // nothing left to do for this file.
        let update_key = engine::hash_key("3c8697e8", "67821d9d");
        let keys = ignore_state(Some(&update_key));

        let reported = scan(&root, &keys).expect("scan");
        let file = &reported.files[0];
        assert!(file.ignored);
        assert_eq!(file.ignored_scope.as_deref(), Some(IGNORE_SCOPE_FIX));
        assert_eq!(file.updated.len(), 1);
        assert!(file.updated[0].ignored);
        assert_eq!(file.updated[0].key, update_key);
        assert_eq!(fs::read(&ini).expect("read"), before, "a scan writes nothing");

        let applied = apply(&root, &[ini_path.clone()], true, &keys).expect("apply");
        assert!(applied.applied.is_empty());
        assert_eq!(applied.ignored, vec![ini_path.clone()]);
        assert_eq!(applied.unchanged, Vec::<String>::new());
        assert_eq!(
            fs::read(&ini).expect("read"),
            before,
            "an ignored recommendation is never written"
        );

        // A whole-mod ignore needs no recommendation key: the file is skipped
        // outright, and the scan still shows what it would have fixed.
        let mut mod_state = store::FixerState::default();
        store::set_ignored(
            &mut mod_state,
            FixerIgnoredEntry {
                folder: "S11Mod".to_string(),
                ini_path: None,
                key: None,
                label: "S11Mod".to_string(),
                ignored_at: 1,
            },
            true,
        );
        let mod_keys = IgnoreSet::from_state(&mod_state);

        let reported = scan(&root, &mod_keys).expect("scan");
        let file = &reported.files[0];
        assert_eq!(file.ignored_scope.as_deref(), Some(IGNORE_SCOPE_MOD));
        assert!(file.updated.iter().all(|change| change.ignored));
        assert!(file.additions.iter().all(|addition| addition.ignored));

        let applied = apply(&root, &[ini_path.clone()], true, &mod_keys).expect("apply");
        assert!(applied.applied.is_empty());
        assert_eq!(applied.ignored, vec![ini_path.clone()]);
        assert_eq!(fs::read(&ini).expect("read"), before);

        // Ignoring only the added section leaves the hash fix working.
        let addition = scan(&root, &IgnoreSet::default()).expect("scan").files[0]
            .additions
            .first()
            .cloned()
            .expect("the fixture adds a section");
        let keys = ignore_state(Some(&addition.key));

        let applied = apply(&root, &[ini_path.clone()], true, &keys).expect("apply");
        assert_eq!(applied.applied.len(), 1);
        let entry = &applied.applied[0];
        assert_eq!(entry.updated.len(), 1);
        assert!(entry.additions.is_empty(), "the ignored addition is not claimed");
        assert_eq!(entry.folder, "S11Mod");
        assert_eq!(entry.character, "Soldier11");
        assert!(entry.fixed_at > 0);
        assert!(entry.backup_path.is_some());

        let after = fs::read_to_string(&ini).expect("read");
        assert!(after.contains("hash = 67821d9d"));
        assert!(!after.contains(&addition.section));

        fs::remove_dir_all(&root).ok();
    }

}
