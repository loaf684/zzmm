use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use serde::Deserialize;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::ini_parser::hotkeys::{can_fire_together, parse_binding, ParsedBinding};
use crate::ini_parser::model::{
    CachedModHotkeys, HotkeyConflict, HotkeyConflictEntry, HotkeyConflictReport, HotkeyView,
    IniDocument, IniFileInfo, ModHotkey,
};
use crate::ini_parser::parser;
use crate::ini_parser::text::{self, Encoding};
use crate::mods::{actions, metadata, scanner};
use crate::state::AppState;

const MAX_DEPTH: usize = 4;
const MAX_FILES: usize = 400;

fn mods_dir(state: &AppState) -> AppResult<PathBuf> {
    let dir = state
        .settings
        .read()
        .map_err(|_| AppError::StateLock)?
        .mods_dir
        .clone();

    dir.map(PathBuf::from).ok_or(AppError::NoModsDir)
}

/// Resolve a frontend-supplied relative path inside one mod folder.
///
/// Only plain relative `.ini` paths are accepted: no absolute paths, no drive
/// letters, no `..` components, no control characters.
fn resolve_ini(mod_root: &Path, relative_path: &str) -> AppResult<PathBuf> {
    let normalized = relative_path.replace('\\', "/");

    if normalized.is_empty() || normalized.len() > 400 {
        return Err(AppError::IniPath(relative_path.to_string()));
    }

    let mut resolved = mod_root.to_path_buf();

    for part in normalized.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(AppError::IniPath(relative_path.to_string()));
        }
        if part.contains(':') || part.chars().any(char::is_control) {
            return Err(AppError::IniPath(relative_path.to_string()));
        }
        resolved.push(part);
    }

    if !normalized.to_ascii_lowercase().ends_with(".ini") {
        return Err(AppError::IniPath(relative_path.to_string()));
    }

    if !resolved.starts_with(mod_root) {
        return Err(AppError::IniPath(relative_path.to_string()));
    }

    let metadata =
        fs::metadata(&resolved).map_err(|_| AppError::IniNotFound(relative_path.to_string()))?;
    if !metadata.is_file() {
        return Err(AppError::IniNotFound(relative_path.to_string()));
    }

    Ok(resolved)
}

fn collect_inis(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf, u64)>) {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return;
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        if out.len() >= MAX_FILES {
            return;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }

        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        let path = entry.path();

        if file_type.is_dir() {
            collect_inis(root, &path, depth + 1, out);
            continue;
        }

        if !file_type.is_file() || !name.to_ascii_lowercase().ends_with(".ini") {
            continue;
        }

        let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        out.push((relative, path, size));
    }
}

/// Every `.ini` under a mod folder, sorted by path. Shared with the 3D preview,
/// which needs `[Resource*]` sections from the same files.
pub(crate) fn collect_ini_files(root: &Path) -> Vec<(String, PathBuf, u64)> {
    let mut found = Vec::new();
    collect_inis(root, root, 0, &mut found);
    found.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    found
}

fn build_document(
    folder_name: &str,
    relative_path: &str,
    path: &Path,
    content: &str,
    encoding: Encoding,
) -> IniDocument {
    let lines = parser::split_lines(content);
    let backup = text::backup_path(path);

    IniDocument {
        mod_folder: folder_name.to_string(),
        relative_path: relative_path.to_string(),
        absolute_path: path.display().to_string(),
        encoding: encoding.label().to_string(),
        content: content.to_string(),
        line_count: lines.len() as u32,
        size: fs::metadata(path).map(|meta| meta.len()).unwrap_or(0),
        sections: parser::parse_section_views(&lines),
        hotkeys: parser::parse_hotkeys(&lines),
        backup_path: if backup.exists() {
            Some(backup.display().to_string())
        } else {
            None
        },
    }
}

fn load_document(folder_name: &str, relative_path: &str, path: &Path) -> AppResult<IniDocument> {
    let (content, encoding) = text::read(path)?;
    Ok(build_document(
        folder_name,
        relative_path,
        path,
        &content,
        encoding,
    ))
}

fn persist(path: &Path, content: &str, encoding: Encoding) -> AppResult<()> {
    text::backup_once(path)?;
    text::write_atomic(path, &text::encode(content, encoding))
}

#[tauri::command]
pub async fn list_mod_ini_files(
    state: State<'_, AppState>,
    folder_name: String,
) -> AppResult<Vec<IniFileInfo>> {
    let mods = mods_dir(&state)?;

    tauri::async_runtime::spawn_blocking(move || -> AppResult<Vec<IniFileInfo>> {
        let root = actions::mod_dir(&mods, &folder_name)?;
        let files = collect_ini_files(&root)
            .into_iter()
            .map(|(relative_path, path, size)| {
                let hotkey_count = text::read(&path)
                    .map(|(content, _)| {
                        if !parser::has_key_sections(&content) {
                            return 0;
                        }
                        parser::parse_hotkeys(&parser::split_lines(&content)).len() as u32
                    })
                    .unwrap_or(0);

                IniFileInfo {
                    name: path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| relative_path.clone()),
                    relative_path,
                    size,
                    hotkey_count,
                }
            })
            .collect();

        Ok(files)
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}

/// Every active binding in one mod, flattened for the installed-mod detail
/// panel. Files ZZMI ignores (`DISABLED*.ini`, `desktop.ini`) and files without
/// a `[Key…]` header are skipped before parsing.
#[tauri::command]
pub async fn get_mod_hotkeys(
    state: State<'_, AppState>,
    folder_name: String,
) -> AppResult<Vec<ModHotkey>> {
    let mods = mods_dir(&state)?;
    let cache = Arc::clone(&state.hotkey_cache);

    tauri::async_runtime::spawn_blocking(move || -> AppResult<Vec<ModHotkey>> {
        let root = actions::mod_dir(&mods, &folder_name)?;
        // Key the cache by the Mods root too: two libraries can contain a
        // folder with the same name.
        let cache_key = format!("{}|{folder_name}", mods.display());
        let mut signature = Vec::new();
        let mut candidates = Vec::new();

        for (relative, path, size) in collect_ini_files(&root) {
            let file_name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();

            if !scanner::is_active_hotkey_ini(&file_name) {
                continue;
            }

            let modified = fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos() as u64)
                .unwrap_or(0);

            signature.push((relative.clone(), size, modified));
            candidates.push((relative, path));
        }

        if let Ok(guard) = cache.lock() {
            if let Some(entry) = guard.get(&cache_key) {
                if entry.signature == signature {
                    return Ok(entry.hotkeys.clone());
                }
            }
        }

        let mut found = Vec::new();

        for (relative, path) in candidates {
            let Ok((content, _encoding)) = text::read(&path) else {
                continue;
            };
            if !parser::has_key_sections(&content) {
                continue;
            }

            let lines = parser::split_lines(&content);
            for hotkey in parser::parse_hotkeys(&lines) {
                if !hotkey.has_key {
                    continue;
                }

                let description = nearby_comment(&lines, (hotkey.line as usize).saturating_sub(1));
                let options = hotkey_options(&hotkey);
                let condition = hotkey
                    .settings
                    .iter()
                    .find(|setting| setting.key.eq_ignore_ascii_case("condition"))
                    .map(|setting| setting.value.clone());

                found.push(ModHotkey {
                    file: relative.clone(),
                    section: hotkey.section,
                    line: hotkey.line,
                    key_setting: hotkey.key_setting,
                    type_setting: hotkey.type_setting,
                    binding: hotkey.binding,
                    action_type: hotkey.action_type,
                    tokens: hotkey.tokens,
                    options,
                    condition,
                    description,
                    variables: hotkey.variables,
                    settings: hotkey.settings,
                });
            }
        }

        if let Ok(mut guard) = cache.lock() {
            guard.insert(
                cache_key,
                CachedModHotkeys {
                    signature,
                    hotkeys: found.clone(),
                },
            );
        }

        Ok(found)
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}

/// Closest comment directly above a section header, skipping blank lines.
fn nearby_comment(lines: &[parser::IniLine], header: usize) -> Option<String> {
    let mut index = header;
    let mut steps = 0;

    while index > 0 && steps < 4 {
        index -= 1;
        steps += 1;

        let trimmed = lines[index].text.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(comment) = trimmed
            .strip_prefix(';')
            .or_else(|| trimmed.strip_prefix('#'))
        {
            let comment = comment.trim();
            if !comment.is_empty() {
                return Some(comment.to_string());
            }
        }

        break;
    }

    None
}

/// Cycle lists are usually written as `$active = 0,1,2`, but some mods use one
/// variable per state. Take the largest option count the section exposes.
fn hotkey_options(hotkey: &HotkeyView) -> u32 {
    let value_options = hotkey
        .variables
        .iter()
        .map(|variable| {
            variable
                .value
                .split(',')
                .filter(|part| !part.trim().is_empty())
                .count() as u32
        })
        .max()
        .unwrap_or(1);

    let variable_count = hotkey.variables.len() as u32;
    let fallback = if hotkey.action_type == "cycle" {
        variable_count
    } else {
        1
    };

    value_options.max(fallback).max(1)
}

#[tauri::command]
pub async fn read_mod_ini(
    state: State<'_, AppState>,
    folder_name: String,
    relative_path: String,
) -> AppResult<IniDocument> {
    let mods = mods_dir(&state)?;

    tauri::async_runtime::spawn_blocking(move || -> AppResult<IniDocument> {
        let root = actions::mod_dir(&mods, &folder_name)?;
        let path = resolve_ini(&root, &relative_path)?;
        load_document(&folder_name, &relative_path, &path)
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}

/// Save the raw editor content, preserving the file's original encoding.
#[tauri::command]
pub async fn save_mod_ini_raw(
    state: State<'_, AppState>,
    folder_name: String,
    relative_path: String,
    content: String,
) -> AppResult<IniDocument> {
    let mods = mods_dir(&state)?;

    tauri::async_runtime::spawn_blocking(move || -> AppResult<IniDocument> {
        let root = actions::mod_dir(&mods, &folder_name)?;
        let path = resolve_ini(&root, &relative_path)?;
        let (_, encoding) = text::read(&path)?;

        persist(&path, &content, encoding)?;
        log::info!("Saved raw INI {}", path.display());

        Ok(build_document(
            &folder_name,
            &relative_path,
            &path,
            &content,
            encoding,
        ))
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}

/// Change one setting inside one `[Section]`, preserving everything else.
#[tauri::command]
pub async fn update_ini_setting(
    state: State<'_, AppState>,
    folder_name: String,
    relative_path: String,
    section: String,
    key: String,
    value: String,
) -> AppResult<IniDocument> {
    if key.trim().is_empty()
        || value.contains('\n')
        || value.contains('\r')
        || key.contains('=')
        || key.chars().any(char::is_control)
    {
        return Err(AppError::IniPath(format!("{section} / {key}")));
    }

    let mods = mods_dir(&state)?;

    tauri::async_runtime::spawn_blocking(move || -> AppResult<IniDocument> {
        let root = actions::mod_dir(&mods, &folder_name)?;
        let path = resolve_ini(&root, &relative_path)?;
        let (content, encoding) = text::read(&path)?;

        let mut lines = parser::split_lines(&content);
        let newline = parser::dominant_newline(&lines);

        let sections = parser::sections(&lines);
        let found_section = sections
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(&section))
            .ok_or_else(|| {
                AppError::IniPath(format!(
                    "Section [{section}] does not exist in {relative_path}"
                ))
            })?;

        match parser::find_setting(&lines, &section, &key) {
            Some(index) => {
                if !parser::replace_value(&mut lines, index, value.trim()) {
                    return Err(AppError::IniPath(format!("{section} / {key}")));
                }
            }
            None => parser::insert_setting(&mut lines, found_section, &key, value.trim(), &newline),
        }

        let updated = parser::join_lines(&lines);
        persist(&path, &updated, encoding)?;
        log::info!("Updated [{section}] {key} in {}", path.display());

        Ok(build_document(
            &folder_name,
            &relative_path,
            &path,
            &updated,
            encoding,
        ))
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}

/// One binding change from the hotkey editor. `None` means "leave this field
/// alone", so editing a key never rewrites `type` and vice versa.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyEdit {
    /// Path relative to the mod folder.
    pub file: String,
    pub section: String,
    /// Spelling of the `key` setting in the file; defaults to `key`.
    pub key_setting: Option<String>,
    /// Spelling of the `type` setting in the file; defaults to `type`.
    pub type_setting: Option<String>,
    pub binding: Option<String>,
    pub action_type: Option<String>,
}

fn apply_hotkey_setting(
    lines: &mut Vec<parser::IniLine>,
    section_name: &str,
    key: &str,
    value: &str,
    newline: &str,
    relative_path: &str,
) -> AppResult<()> {
    if let Some(index) = parser::find_setting(lines, section_name, key) {
        if parser::replace_value(lines, index, value.trim()) {
            return Ok(());
        }
        return Err(AppError::IniPath(format!("{section_name} / {key}")));
    }

    let section = parser::sections(lines)
        .into_iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(section_name))
        .ok_or_else(|| {
            AppError::IniPath(format!(
                "Section [{section_name}] does not exist in {relative_path}"
            ))
        })?;

    parser::insert_setting(lines, &section, key, value.trim(), newline);
    Ok(())
}

/// Apply every pending hotkey edit in one pass.
///
/// The panel used to issue one `update_ini_setting` call per changed field,
/// re-reading and rewriting the same file for each field. This validates the
/// whole batch up front, then reads, patches and atomically writes each INI
/// exactly once, so saving six bindings in one file costs one write.
#[tauri::command]
pub async fn apply_mod_hotkey_edits(
    state: State<'_, AppState>,
    folder_name: String,
    edits: Vec<HotkeyEdit>,
) -> AppResult<u32> {
    if edits.is_empty() {
        return Ok(0);
    }

    for edit in &edits {
        if edit.section.trim().is_empty()
            || edit.section.contains('\n')
            || edit.section.contains('\r')
        {
            return Err(AppError::IniPath(edit.file.clone()));
        }

        if let Some(binding) = edit.binding.as_deref() {
            let binding = binding.trim();
            if binding.is_empty() || binding.contains('\n') || binding.contains('\r') {
                return Err(AppError::IniPath(format!(
                    "{} / {}",
                    edit.file, edit.section
                )));
            }
        }

        if let Some(action) = edit.action_type.as_deref() {
            let action = action.trim();
            if action.is_empty()
                || !action
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
            {
                return Err(AppError::IniPath(format!(
                    "{} / {}",
                    edit.file, edit.section
                )));
            }
        }
    }

    let mods = mods_dir(&state)?;
    let edit_count = edits.len() as u32;

    tauri::async_runtime::spawn_blocking(move || -> AppResult<u32> {
        let root = actions::mod_dir(&mods, &folder_name)?;

        let mut by_file: BTreeMap<String, Vec<HotkeyEdit>> = BTreeMap::new();
        for edit in edits {
            by_file.entry(edit.file.clone()).or_default().push(edit);
        }

        let mut written = 0u32;

        for (relative, file_edits) in by_file {
            let path = resolve_ini(&root, &relative)?;
            let (content, encoding) = text::read(&path)?;
            let mut lines = parser::split_lines(&content);
            let newline = parser::dominant_newline(&lines);

            for edit in &file_edits {
                if let Some(binding) = edit.binding.as_deref() {
                    apply_hotkey_setting(
                        &mut lines,
                        &edit.section,
                        edit.key_setting.as_deref().unwrap_or("key"),
                        binding,
                        &newline,
                        &relative,
                    )?;
                }

                if let Some(action) = edit.action_type.as_deref() {
                    apply_hotkey_setting(
                        &mut lines,
                        &edit.section,
                        edit.type_setting.as_deref().unwrap_or("type"),
                        action,
                        &newline,
                        &relative,
                    )?;
                }
            }

            let updated = parser::join_lines(&lines);
            if updated != content {
                persist(&path, &updated, encoding)?;
                written += 1;
            }
        }

        log::info!("Applied {edit_count} hotkey edit(s) to {written} INI file(s) in {folder_name}");

        Ok(written)
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}

/// Group bindings from all enabled mods that can fire on the same key press.
#[tauri::command]
pub async fn scan_hotkey_conflicts(state: State<'_, AppState>) -> AppResult<HotkeyConflictReport> {
    let mods = mods_dir(&state)?;

    tauri::async_runtime::spawn_blocking(move || -> AppResult<HotkeyConflictReport> {
        let mut candidates: Vec<(HotkeyConflictEntry, ParsedBinding)> = Vec::new();
        let mut scanned_mods = 0u32;
        let mut scanned_files = 0u32;

        let entries = fs::read_dir(&mods)?;

        for entry in entries.flatten() {
            let folder_name = entry.file_name().to_string_lossy().to_string();
            if folder_name.starts_with('.') {
                continue;
            }
            if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                continue;
            }

            // Only enabled mods load into the game.
            let (disabled, base_name) = scanner::split_disabled(&folder_name);
            if disabled {
                continue;
            }

            scanned_mods += 1;

            let mod_root = entry.path();
            let meta = metadata::read(&mod_root);
            let display_name = meta
                .as_ref()
                .and_then(|meta| meta.name.clone())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| base_name.clone());

            for (relative, path, _size) in collect_ini_files(&mod_root) {
                let file_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();

                // Same rules as the detail panel: ZZMI ignores these files, so
                // their bindings can neither fire nor conflict.
                if !scanner::is_active_hotkey_ini(&file_name) {
                    continue;
                }

                let Ok((content, _encoding)) = text::read(&path) else {
                    continue;
                };
                if !parser::has_key_sections(&content) {
                    continue;
                }

                scanned_files += 1;

                for hotkey in parser::parse_hotkeys(&parser::split_lines(&content)) {
                    if !hotkey.has_key {
                        continue;
                    }

                    let condition = hotkey
                        .settings
                        .iter()
                        .find(|setting| setting.key.eq_ignore_ascii_case("condition"))
                        .map(|setting| setting.value.clone());

                    let binding = parse_binding(&hotkey.binding).with_condition(condition.clone());
                    if binding.is_empty() {
                        continue;
                    }

                    candidates.push((
                        HotkeyConflictEntry {
                            folder_name: folder_name.clone(),
                            base_name: base_name.clone(),
                            display_name: display_name.clone(),
                            file: relative.clone(),
                            section: hotkey.section,
                            line: hotkey.line,
                            binding: hotkey.binding,
                            action_type: hotkey.action_type,
                            condition,
                        },
                        binding,
                    ));
                }
            }
        }

        let total_hotkeys = candidates.len() as u32;
        Ok(HotkeyConflictReport {
            scanned_mods,
            scanned_files,
            hotkeys: total_hotkeys,
            conflicts: build_conflicts(&candidates),
        })
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}

fn build_conflicts(candidates: &[(HotkeyConflictEntry, ParsedBinding)]) -> Vec<HotkeyConflict> {
    let mut conflicts = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    for (index, (entry, binding)) in candidates.iter().enumerate() {
        let Some(key) = binding.key.clone() else {
            continue;
        };

        // Collect every binding that can fire together with this one.
        let mut group: Vec<usize> = (0..candidates.len())
            .filter(|other| *other != index && can_fire_together(binding, &candidates[*other].1))
            .collect();

        if group.is_empty() {
            continue;
        }
        group.push(index);

        let distinct_mods: BTreeSet<&str> = group
            .iter()
            .map(|member| candidates[*member].0.folder_name.as_str())
            .collect();

        // A single mod using one key twice is its own problem, but the feature
        // is about mods fighting each other.
        if distinct_mods.len() < 2 {
            continue;
        }

        group.sort_unstable();

        let fingerprint = group
            .iter()
            .map(|member| {
                let entry = &candidates[*member].0;
                format!("{}|{}|{}", entry.folder_name, entry.file, entry.section)
            })
            .collect::<Vec<_>>()
            .join("#");

        if !seen.insert(fingerprint) {
            continue;
        }

        conflicts.push(HotkeyConflict {
            key,
            binding: entry.binding.clone(),
            entries: group
                .iter()
                .map(|member| candidates[*member].0.clone())
                .collect(),
        });
    }

    conflicts.sort_by(|a, b| {
        b.entries
            .len()
            .cmp(&a.entries.len())
            .then_with(|| a.key.cmp(&b.key))
    });

    conflicts
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "; Cycle the outfit\r\n; second line\r\n[KeySwap]\r\nkey = ctrl VK_1\r\ntype = cycle\r\n$active = 0,1,2\r\n\r\n[TextureOverrideX]\r\nhash = abc\r\n";

    #[test]
    fn finds_the_closest_comment_above_a_section() {
        let lines = parser::split_lines(SAMPLE);
        let hotkeys = parser::parse_hotkeys(&lines);
        let header = (hotkeys[0].line as usize) - 1;

        assert_eq!(
            nearby_comment(&lines, header).as_deref(),
            Some("second line")
        );
    }

    #[test]
    fn counts_cycle_options_from_comma_lists_and_variables() {
        let lines = parser::split_lines(SAMPLE);
        let hotkeys = parser::parse_hotkeys(&lines);
        assert_eq!(hotkey_options(&hotkeys[0]), 3);

        let separate =
            parser::split_lines("[KeyHair]\nkey = VK_2\ntype = cycle\n$one = 0\n$two = 1\n");
        let separate = parser::parse_hotkeys(&separate);
        assert_eq!(hotkey_options(&separate[0]), 2);
    }

    #[test]
    fn patches_several_fields_in_one_section() {
        let mut lines = parser::split_lines("[KeySwap]\nkey = VK_1\ntype = toggle\n");

        apply_hotkey_setting(&mut lines, "keyswap", "key", "ctrl VK_2", "\n", "a.ini")
            .expect("binding edit");
        apply_hotkey_setting(&mut lines, "KeySwap", "type", "hold", "\n", "a.ini")
            .expect("type edit");

        let text = parser::join_lines(&lines);
        assert!(text.contains("key = ctrl VK_2"), "{text}");
        assert!(text.contains("type = hold"), "{text}");
        assert_eq!(text.matches("type =").count(), 1, "{text}");

        apply_hotkey_setting(
            &mut lines,
            "KeySwap",
            "condition",
            "$active == 1",
            "\n",
            "a.ini",
        )
        .expect("new setting");
        assert!(parser::join_lines(&lines).contains("condition = $active == 1"));
    }

    #[test]
    fn rejects_edits_for_missing_sections() {
        let mut lines = parser::split_lines("[KeySwap]\nkey = VK_1\n");
        let error = apply_hotkey_setting(&mut lines, "KeyNope", "key", "VK_2", "\n", "a.ini");
        assert!(error.is_err());
    }

    #[test]
    fn prefilters_files_without_key_sections() {
        assert!(parser::has_key_sections("[KeySwap]\nkey = VK_1\n"));
        assert!(!parser::has_key_sections(
            "[TextureOverrideX]\nhash = abc\n"
        ));
    }

    #[test]
    fn hotkey_conflicts_ignore_mutually_exclusive_conditions() {
        let entry1 = HotkeyConflictEntry {
            folder_name: "ModA".to_string(),
            base_name: "ModA".to_string(),
            display_name: "Mod A".to_string(),
            file: "a.ini".to_string(),
            section: "Key1".to_string(),
            line: 1,
            binding: "VK_F1".to_string(),
            action_type: "activate".to_string(),
            condition: Some("$character == 1".to_string()),
        };
        let binding1 = parse_binding("VK_F1").with_condition(Some("$character == 1".to_string()));

        let entry2 = HotkeyConflictEntry {
            folder_name: "ModB".to_string(),
            base_name: "ModB".to_string(),
            display_name: "Mod B".to_string(),
            file: "b.ini".to_string(),
            section: "Key1".to_string(),
            line: 1,
            binding: "VK_F1".to_string(),
            action_type: "activate".to_string(),
            condition: Some("$character == 2".to_string()),
        };
        let binding2 = parse_binding("VK_F1").with_condition(Some("$character == 2".to_string()));

        let candidates = vec![(entry1, binding1), (entry2, binding2)];
        let conflicts = build_conflicts(&candidates);
        assert!(
            conflicts.is_empty(),
            "mutually exclusive conditions should not conflict"
        );
    }

    #[test]
    fn hotkey_conflicts_report_overlapping_conditions() {
        let entry1 = HotkeyConflictEntry {
            folder_name: "ModA".to_string(),
            base_name: "ModA".to_string(),
            display_name: "Mod A".to_string(),
            file: "a.ini".to_string(),
            section: "Key1".to_string(),
            line: 1,
            binding: "VK_F1".to_string(),
            action_type: "activate".to_string(),
            condition: Some("$character == 1".to_string()),
        };
        let binding1 = parse_binding("VK_F1").with_condition(Some("$character == 1".to_string()));

        let entry2 = HotkeyConflictEntry {
            folder_name: "ModB".to_string(),
            base_name: "ModB".to_string(),
            display_name: "Mod B".to_string(),
            file: "b.ini".to_string(),
            section: "Key1".to_string(),
            line: 1,
            binding: "VK_F1".to_string(),
            action_type: "activate".to_string(),
            condition: Some("$character == 1".to_string()),
        };
        let binding2 = parse_binding("VK_F1").with_condition(Some("$character == 1".to_string()));

        let candidates = vec![(entry1, binding1), (entry2, binding2)];
        let conflicts = build_conflicts(&candidates);
        assert_eq!(conflicts.len(), 1, "overlapping conditions should conflict");
    }
}
