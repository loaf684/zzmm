use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IniFileInfo {
    /// Path relative to the mod folder, always with `/` separators.
    pub relative_path: String,
    pub name: String,
    pub size: u64,
    pub hotkey_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IniSectionView {
    pub name: String,
    /// 1-based line of the `[Section]` header.
    pub line: u32,
    pub setting_count: u32,
    /// True for `[Key*]` sections.
    pub is_hotkey: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IniVariable {
    pub name: String,
    pub value: String,
    pub line: u32,
}

/// Any `key = value` line that is not `key`, `type` or a `$variable`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IniSettingValue {
    pub key: String,
    pub value: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyView {
    pub section: String,
    /// 1-based line of the section header.
    pub line: u32,
    /// Actual spelling of the `key` setting in the file (`key`, `Key`, …).
    pub key_setting: Option<String>,
    /// Actual spelling of the `type` setting, if the section has one.
    pub type_setting: Option<String>,
    /// Raw binding text, e.g. `ctrl alt VK_F1`.
    pub binding: String,
    /// `activate` (default), `hold`, `toggle`, `cycle`, or `custom`.
    pub action_type: String,
    pub has_key: bool,
    /// Uppercased tokens of the binding, for chips in the UI.
    pub tokens: Vec<String>,
    pub variables: Vec<IniVariable>,
    pub settings: Vec<IniSettingValue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IniDocument {
    pub mod_folder: String,
    pub relative_path: String,
    pub absolute_path: String,
    /// `utf-8`, `utf-8-bom`, `utf-16le` or `latin-1`.
    pub encoding: String,
    /// Full file text; this is what the raw Monaco editor edits.
    pub content: String,
    pub line_count: u32,
    pub size: u64,
    pub sections: Vec<IniSectionView>,
    pub hotkeys: Vec<HotkeyView>,
    /// Present once a save has created `<file>.ini.bak`.
    pub backup_path: Option<String>,
}

/// One active binding, flattened across every INI file in a single mod. This is
/// what the installed-mod detail panel renders as keyboard badges.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModHotkey {
    /// Path relative to the mod folder, always with `/` separators.
    pub file: String,
    pub section: String,
    /// 1-based line of the `[Section]` header.
    pub line: u32,
    /// Actual spelling of the `key` setting in the file (`key`, `Key`, …).
    pub key_setting: Option<String>,
    /// Actual spelling of the `type` setting, if the section has one.
    pub type_setting: Option<String>,
    pub binding: String,
    pub action_type: String,
    pub tokens: Vec<String>,
    /// Number of values offered by the binding (cycle lists, `$variables`).
    pub options: u32,
    /// `condition = …` line, when the section has one.
    pub condition: Option<String>,
    /// Nearest comment above the section, when there is one.
    pub description: Option<String>,
    pub variables: Vec<IniVariable>,
    pub settings: Vec<IniSettingValue>,
}

/// Session cache for one mod's hotkey report. The signature holds each INI's
/// relative path, size and modified time, so any edit invalidates the entry.
#[derive(Debug, Clone, Default)]
pub struct CachedModHotkeys {
    pub signature: Vec<(String, u64, u64)>,
    pub hotkeys: Vec<ModHotkey>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyConflictEntry {
    pub folder_name: String,
    pub base_name: String,
    pub display_name: String,
    pub file: String,
    pub section: String,
    pub line: u32,
    pub binding: String,
    pub action_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyConflict {
    /// Canonical key token, e.g. `VK_F1`.
    pub key: String,
    /// Binding as printed in the INI, for context.
    pub binding: String,
    pub entries: Vec<HotkeyConflictEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyConflictReport {
    pub scanned_mods: u32,
    pub scanned_files: u32,
    pub hotkeys: u32,
    pub conflicts: Vec<HotkeyConflict>,
}
