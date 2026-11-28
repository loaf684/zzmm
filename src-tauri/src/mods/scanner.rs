use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::ini_parser::parser;
use crate::mods::characters;
use crate::mods::metadata;
use crate::mods::model::{
    HashConflict, HashConflictPart, InstalledMod, ModScanResult, ScanProgress,
};

pub const DISABLED_PREFIX: &str = "DISABLED_";

/// ZZMI ignores `DISABLED*.ini` files, and `desktop.ini` is shell metadata.
/// Only INIs that pass this check contribute active bindings or conflicts.
pub fn is_active_hotkey_ini(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    !lower.starts_with("disabled") && lower != "desktop.ini"
}

const MAX_INI_BYTES: u64 = 8 * 1024 * 1024;
const MAX_INI_DEPTH: usize = 4;
const MAX_THUMBNAIL_BYTES: u64 = 20 * 1024 * 1024;
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];
const THUMBNAIL_NAMES: &[&str] = &["preview", "thumbnail", "thumb", "cover", "icon"];

struct ScannedHash {
    hash: String,
    file: String,
    section: String,
    line: u32,
}

struct ScannedMod {
    installed: InstalledMod,
    hash_refs: Vec<ScannedHash>,
}

/// Split the ZZMI `DISABLED_` prefix without regard to casing (folders may be
/// renamed by other tools).
pub fn split_disabled(folder_name: &str) -> (bool, String) {
    match folder_name.get(..DISABLED_PREFIX.len()) {
        Some(prefix) if prefix.eq_ignore_ascii_case(DISABLED_PREFIX) => {
            (true, folder_name[DISABLED_PREFIX.len()..].to_string())
        }
        _ => (false, folder_name.to_string()),
    }
}

/// Scan the Mods folder. `on_progress` is called with coarse progress so the
/// command layer can forward it to the UI without blocking the webview.
pub fn scan<F>(mods_dir: &Path, mut on_progress: F) -> AppResult<ModScanResult>
where
    F: FnMut(ScanProgress),
{
    if !mods_dir.is_dir() {
        return Err(AppError::ModsDirMissing(mods_dir.display().to_string()));
    }

    let mut folders = read_mod_folders(mods_dir)?;
    folders.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));

    let total = folders.len() as u32;
    on_progress(ScanProgress {
        scanned: 0,
        total,
        folder_name: String::new(),
    });

    let mut scanned = Vec::with_capacity(folders.len());

    for (index, (folder_name, base_name, enabled)) in folders.into_iter().enumerate() {
        let path = mods_dir.join(&folder_name);
        scanned.push(inspect_mod(&path, &folder_name, &base_name, enabled));

        on_progress(ScanProgress {
            scanned: (index as u32) + 1,
            total,
            folder_name,
        });
    }

    let conflicts = build_conflicts(&scanned);
    let conflicted: HashSet<&str> = conflicts
        .iter()
        .flat_map(|conflict| conflict.mods.iter())
        .map(|part| part.folder_name.as_str())
        .collect();

    for scanned_mod in &mut scanned {
        scanned_mod.installed.has_conflict =
            conflicted.contains(scanned_mod.installed.folder_name.as_str());
    }

    let mods: Vec<InstalledMod> = scanned
        .into_iter()
        .map(|scanned_mod| scanned_mod.installed)
        .collect();

    Ok(ModScanResult {
        mods_dir: mods_dir.display().to_string(),
        enabled_count: mods.iter().filter(|item| item.enabled).count() as u32,
        disabled_count: mods.iter().filter(|item| !item.enabled).count() as u32,
        unlinked_count: mods.iter().filter(|item| !item.linked).count() as u32,
        mods,
        conflicts,
    })
}

fn read_mod_folders(mods_dir: &Path) -> AppResult<Vec<(String, String, bool)>> {
    let entries = fs::read_dir(mods_dir)?;
    let mut folders = Vec::new();

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

        folders.push((folder_name, base_name, !is_disabled));
    }

    Ok(folders)
}

fn inspect_mod(path: &Path, folder_name: &str, base_name: &str, enabled: bool) -> ScannedMod {
    let metadata = metadata::read(path);
    let display_name = display_name(&metadata, base_name);

    let thumbnail_path = find_thumbnail(path);
    // Only GameBanana-backed mods are "linked": `.zzmm.json` may also exist
    // purely because the user saved notes or a custom preview.
    let linked = metadata
        .as_ref()
        .map_or(false, |meta| meta.game_banana_mod_id.is_some());

    let (hash_refs, hotkey_count, name_hints) = collect_ini_info(path, path, 0);

    // Folder names can be generic while the INIs inside still name the
    // character, so pass every hint through the roster matcher.
    let character =
        characters::guess_with_context(metadata.as_ref(), base_name, &display_name, &name_hints);

    let mut hashes: Vec<String> = hash_refs
        .iter()
        .map(|hash_ref| hash_ref.hash.clone())
        .collect();
    hashes.sort();
    hashes.dedup();

    ScannedMod {
        installed: InstalledMod {
            folder_name: folder_name.to_string(),
            base_name: base_name.to_string(),
            display_name,
            character,
            enabled,
            linked,
            thumbnail_path,
            hashes,
            hotkey_count,
            has_conflict: false,
            metadata,
        },
        hash_refs,
    }
}

/// The name shown for a mod: the sidecar's, or the folder-derived base name.
fn display_name(metadata: &Option<metadata::ModMetadata>, base_name: &str) -> String {
    metadata
        .as_ref()
        .and_then(|meta| meta.name.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(base_name)
        .to_string()
}

/// The confident character of a mod folder that is already on disk, or `None`
/// when it cannot be pinned to a known ZZZ character. Used by the installer so
/// a second mod for the same character lands disabled. Only INI *names* are
/// read here, never their bodies, so classifying an existing library stays
/// cheap.
pub fn installed_character(mod_dir: &Path, base_name: &str) -> Option<&'static str> {
    let metadata = metadata::read(mod_dir);
    let display_name = display_name(&metadata, base_name);
    let hints = collect_name_hints(mod_dir, mod_dir, 0);

    characters::confident_character(metadata.as_ref(), base_name, &display_name, &hints)
}

/// The confident character of mod roots that are about to be installed under
/// `base_name`. `hint` is a character the download source already knows
/// (GameBanana's character subcategory) before any `.zzmm.json` has been
/// written.
pub fn staged_character(
    base_name: &str,
    roots: &[PathBuf],
    hint: Option<&str>,
) -> Option<&'static str> {
    let metadata = roots.iter().find_map(|root| metadata::read(root));
    let display_name = display_name(&metadata, base_name);

    let mut hints = Vec::new();
    for root in roots {
        hints.extend(collect_name_hints(root, root, 0));
    }
    if let Some(hint) = hint {
        hints.push(hint.to_string());
    }

    characters::confident_character(metadata.as_ref(), base_name, &display_name, &hints)
}

/// Only look at the mod root: nested images are usually textures, not previews.
fn find_thumbnail(mod_dir: &Path) -> Option<String> {
    let entries = fs::read_dir(mod_dir).ok()?;
    let mut named: Option<(usize, String)> = None;
    let mut fallback: Option<String> = None;

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }

        let path = entry.path();
        let Some(extension) = path.extension() else {
            continue;
        };
        let extension = extension.to_string_lossy().to_ascii_lowercase();
        if !IMAGE_EXTENSIONS.contains(&extension.as_str()) {
            continue;
        }

        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.len() > MAX_THUMBNAIL_BYTES {
            continue;
        }

        let display_path = path.to_string_lossy().to_string();
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();

        let rank = THUMBNAIL_NAMES
            .iter()
            .position(|name| stem == *name || stem.starts_with(name));

        match rank {
            Some(rank) if named.as_ref().map_or(true, |(best, _)| rank < *best) => {
                named = Some((rank, display_path));
            }
            Some(_) => {}
            None => {
                if fallback.is_none() {
                    fallback = Some(display_path);
                }
            }
        }
    }

    named.map(|(_, path)| path).or(fallback)
}

/// Read every INI once and collect both texture hashes and the number of active
/// `[Key*]` bindings. This is what powers the card's keyboard badge without a
/// second pass over the Mods folder.
fn collect_ini_info(dir: &Path, root: &Path, depth: usize) -> (Vec<ScannedHash>, u32, Vec<String>) {
    let mut hashes = Vec::new();
    let mut hotkey_count = 0u32;
    let mut name_hints = Vec::new();

    if depth > MAX_INI_DEPTH {
        return (hashes, hotkey_count, name_hints);
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return (hashes, hotkey_count, name_hints);
    };

    let mut entries: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
        .collect();
    entries.sort();

    for path in entries {
        let Ok(file_type) = fs::symlink_metadata(&path).map(|metadata| metadata.file_type()) else {
            continue;
        };

        if file_type.is_dir() {
            let (nested_hashes, nested_hotkeys, nested_hints) =
                collect_ini_info(&path, root, depth + 1);
            hashes.extend(nested_hashes);
            hotkey_count = hotkey_count.saturating_add(nested_hotkeys);
            name_hints.extend(nested_hints);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if !is_ini_file(&path) {
            continue;
        }

        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        // ZZMI ignores `DISABLED*.ini`, and desktop.ini is shell metadata.
        let hotkeys_active = is_active_hotkey_ini(&file_name);

        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.len() > MAX_INI_BYTES {
            log::warn!(
                "Skipping oversized INI ({} bytes): {}",
                metadata.len(),
                path.display()
            );
            continue;
        }

        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let text = decode_ini(&bytes);
        let relative = path
            .strip_prefix(root)
            .map(|value| value.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.to_string_lossy().to_string());

        // Character hints. Creators often ship a generic folder (`Outfit 01`)
        // but name the INI after the character (`Dialyn.ini`), or put the name
        // on a sub-folder. Without these the roster matcher only ever saw the
        // folder itself and fell back to "Uncategorized".
        name_hints.extend(ini_name_hints(&path, root));

        parse_ini_hashes(&text, &relative, &mut hashes);

        if hotkeys_active && parser::has_key_sections(&text) {
            let lines = parser::split_lines(&text);
            hotkey_count = hotkey_count.saturating_add(
                parser::parse_hotkeys(&lines)
                    .iter()
                    .filter(|hotkey| hotkey.has_key)
                    .count() as u32,
            );
        }
    }

    (hashes, hotkey_count, name_hints)
}

/// True for a `.ini` file, whatever the case of its extension.
fn is_ini_file(path: &Path) -> bool {
    path.extension()
        .map(|extension| extension.to_string_lossy().eq_ignore_ascii_case("ini"))
        .unwrap_or(false)
}

/// Character hints one INI contributes: its own name, and — for a nested INI —
/// the folder holding it.
fn ini_name_hints(path: &Path, root: &Path) -> Vec<String> {
    let mut hints = Vec::new();

    if let Some(stem) = path.file_stem() {
        let stem = stem.to_string_lossy();
        if !stem.trim().is_empty() {
            hints.push(stem.to_string());
        }
    }

    if let Some(parent) = path.parent() {
        if parent != root {
            if let Some(folder) = parent.file_name() {
                let folder = folder.to_string_lossy();
                if !folder.trim().is_empty() {
                    hints.push(folder.to_string());
                }
            }
        }
    }

    hints
}

/// Collect only the character hints (INI names and their folders) from a mod
/// tree, without opening a single INI. Mirrors the INI walk in
/// [`collect_ini_info`], which gathers the same hints while it reads each body.
fn collect_name_hints(dir: &Path, root: &Path, depth: usize) -> Vec<String> {
    let mut hints = Vec::new();

    if depth > MAX_INI_DEPTH {
        return hints;
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return hints;
    };

    let mut entries: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
        .collect();
    entries.sort();

    for path in entries {
        let Ok(file_type) = fs::symlink_metadata(&path).map(|metadata| metadata.file_type()) else {
            continue;
        };

        if file_type.is_dir() {
            hints.extend(collect_name_hints(&path, root, depth + 1));
            continue;
        }
        if !file_type.is_file() || !is_ini_file(&path) {
            continue;
        }

        hints.extend(ini_name_hints(&path, root));
    }

    hints
}

fn decode_ini(bytes: &[u8]) -> String {
    // 3DMigoto INIs are usually UTF-8/ANSI, but UTF-16LE files do show up.
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }

    String::from_utf8_lossy(bytes).into_owned()
}

fn parse_ini_hashes(text: &str, file: &str, out: &mut Vec<ScannedHash>) {
    let mut section = String::new();

    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') {
            if let Some(end) = line.find(']') {
                section = line[1..end].trim().to_string();
            }
            continue;
        }

        if !section.to_ascii_lowercase().starts_with("textureoverride") {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("hash") {
            continue;
        }

        let token = value
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .split_whitespace()
            .next()
            .unwrap_or("");
        let token = token.trim_start_matches("0x").trim_start_matches("0X");
        let hash = token.to_ascii_lowercase();

        if hash.is_empty() || !hash.chars().all(|character| character.is_ascii_hexdigit()) {
            continue;
        }

        out.push(ScannedHash {
            hash,
            file: file.to_string(),
            section: section.clone(),
            line: (index + 1) as u32,
        });
    }
}

fn build_conflicts(scanned: &[ScannedMod]) -> Vec<HashConflict> {
    let mut by_hash: HashMap<&str, Vec<(&ScannedMod, &ScannedHash)>> = HashMap::new();

    // Only enabled mods affect load order.
    for scanned_mod in scanned.iter().filter(|item| item.installed.enabled) {
        for hash_ref in &scanned_mod.hash_refs {
            by_hash
                .entry(hash_ref.hash.as_str())
                .or_default()
                .push((scanned_mod, hash_ref));
        }
    }

    let mut conflicts = Vec::new();

    for (hash, refs) in by_hash {
        let distinct_mods: HashSet<&str> = refs
            .iter()
            .map(|(scanned_mod, _)| scanned_mod.installed.folder_name.as_str())
            .collect();
        if distinct_mods.len() < 2 {
            continue;
        }

        let mut mods: Vec<HashConflictPart> = refs
            .iter()
            .map(|(scanned_mod, hash_ref)| HashConflictPart {
                folder_name: scanned_mod.installed.folder_name.clone(),
                base_name: scanned_mod.installed.base_name.clone(),
                display_name: scanned_mod.installed.display_name.clone(),
                file: hash_ref.file.clone(),
                section: hash_ref.section.clone(),
                line: hash_ref.line,
            })
            .collect();

        mods.sort_by(|a, b| {
            (&a.folder_name, &a.file, &a.section, a.line).cmp(&(
                &b.folder_name,
                &b.file,
                &b.section,
                b.line,
            ))
        });
        mods.dedup_by(|a, b| {
            a.folder_name == b.folder_name
                && a.file == b.file
                && a.section == b.section
                && a.line == b.line
        });
        mods.sort_by(|a, b| {
            a.display_name
                .to_lowercase()
                .cmp(&b.display_name.to_lowercase())
        });

        conflicts.push(HashConflict {
            hash: hash.to_string(),
            mods,
        });
    }

    conflicts.sort_by(|a, b| {
        b.mods
            .len()
            .cmp(&a.mods.len())
            .then_with(|| a.hash.cmp(&b.hash))
    });
    conflicts
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_mods_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("zzmm-scan-test-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        let mut file = fs::File::create(path).expect("create file");
        file.write_all(contents.as_bytes()).expect("write file");
    }

    #[test]
    fn splits_disabled_prefix_case_insensitively() {
        assert_eq!(split_disabled("DISABLED_Foo"), (true, "Foo".to_string()));
        assert_eq!(split_disabled("disabled_Foo"), (true, "Foo".to_string()));
        assert_eq!(split_disabled("Foo"), (false, "Foo".to_string()));
    }

    #[test]
    fn ignores_disabled_and_shell_inis_for_hotkeys() {
        assert!(is_active_hotkey_ini("mod.ini"));
        assert!(is_active_hotkey_ini("Keys.INI"));
        assert!(!is_active_hotkey_ini("DISABLED_extra.ini"));
        assert!(!is_active_hotkey_ini("disabled_extra.ini"));
        assert!(!is_active_hotkey_ini("Desktop.ini"));
        assert!(!is_active_hotkey_ini("desktop.ini"));
    }

    #[test]
    fn scans_mods_metadata_and_hash_conflicts() {
        let root = temp_mods_dir();

        write_file(
            &root.join("[Ellen Joe] Streetwear/mod.ini"),
            "[TextureOverrideEllenBody]\nhash = 1A2B3C4D\n;\nhash = deadbeef\n\n[Other]\nhash = ffffffff\n",
        );
        write_file(
            &root.join("[Ellen Joe] Streetwear/keys.ini"),
            "; Swap outfit\n[KeySwap]\nkey = ctrl VK_1\ntype = cycle\n$active = 0,1,2\n",
        );
        // ZZMI ignores this file, so its binding must not be counted.
        write_file(
            &root.join("[Ellen Joe] Streetwear/DISABLED_extra.ini"),
            "[KeyIgnored]\nkey = VK_F9\n",
        );
        write_file(&root.join("[Ellen Joe] Streetwear/preview.png"), "stub");

        write_file(
            &root.join("Nicole_Test/mod.ini"),
            "[TextureOverrideNicole]\n  hash=1a2b3c4d ; shared\n",
        );

        write_file(
            &root.join("DISABLED_Yixuan Manual/mod.ini"),
            "[TextureOverrideYixuan]\nhash = feedface\n",
        );
        write_file(
            &root.join("DISABLED_Yixuan Manual/.zzmm.json"),
            r#"{"gameBananaModId":123,"gameBananaFileId":9,"name":"Yixuan Red","character":"Yixuan","version":"1.0"}"#,
        );

        let result = scan(&root, |_| {}).expect("scan");

        assert_eq!(result.mods.len(), 3);
        assert_eq!(result.enabled_count, 2);
        assert_eq!(result.disabled_count, 1);
        assert_eq!(result.unlinked_count, 2);

        let ellen = result
            .mods
            .iter()
            .find(|item| item.folder_name.contains("Ellen"))
            .expect("ellen mod");
        assert!(ellen.enabled);
        assert!(!ellen.linked);
        assert_eq!(ellen.character, "Ellen Joe");
        assert_eq!(ellen.hashes.len(), 2);
        assert_eq!(ellen.hotkey_count, 1);
        assert!(ellen.has_conflict);
        assert!(ellen.thumbnail_path.is_some());

        let nicole = result
            .mods
            .iter()
            .find(|item| item.folder_name == "Nicole_Test")
            .expect("nicole mod");
        assert_eq!(nicole.character, "Nicole Demara");

        let yixuan = result
            .mods
            .iter()
            .find(|item| item.folder_name.starts_with("DISABLED_"))
            .expect("yixuan mod");
        assert!(!yixuan.enabled);
        assert!(yixuan.linked);
        assert_eq!(yixuan.display_name, "Yixuan Red");
        assert_eq!(yixuan.character, "Yixuan");
        assert_eq!(yixuan.hashes, vec!["feedface".to_string()]);

        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(result.conflicts[0].hash, "1a2b3c4d");
        assert_eq!(result.conflicts[0].mods.len(), 2);

        fs::remove_dir_all(&root).ok();
    }
}
