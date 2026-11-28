//! Data shapes crossing the IPC bridge for the mod fixer.
//!
//! Field names are fixed by the UI that consumes them (`fixerScan`,
//! `fixerApply`, `fixerRollback`); extra fields may be added, existing ones must
//! keep their meaning.

use serde::{Deserialize, Serialize};

/// `ignoredScope` values: the whole mod folder, one INI, or one recommendation.
pub const IGNORE_SCOPE_MOD: &str = "mod";
pub const IGNORE_SCOPE_FILE: &str = "file";
pub const IGNORE_SCOPE_FIX: &str = "fix";

/// One `hash = <old>` line rewritten to a newer hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HashUpdate {
    pub from: String,
    pub to: String,
    /// Which fix this came from, e.g. `1.0: Soldier11 HeadA Diffuse 1024p Hash`.
    pub label: String,
    /// Stable identity of this recommendation, `hash:<from>><to>`; the ignore
    /// list keys a per-recommendation ignore on it.
    pub key: String,
    /// True when the user ignored exactly this recommendation: it is reported
    /// so it stays visible, but nothing is written for it.
    pub ignored: bool,
}

/// One section added under a different hash (the "multiply"/"add if missing"
/// fixes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionAddition {
    /// Section header as written, `[TextureOverride...]`.
    pub section: String,
    /// Hash the added section keys off.
    pub hash: String,
    pub label: String,
    /// Stable identity of this recommendation, `section:<hash>:<section>`.
    pub key: String,
    /// True when the user ignored exactly this recommendation.
    pub ignored: bool,
}

/// A command ZZMM will not run. Never silently dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsupportedCommand {
    pub command: String,
    pub hash: String,
    pub label: String,
    /// `requires the buffer pass`, `unknown command`, each reported verbatim
    /// so the UI can explain *why* a mod is only partly fixed.
    pub reason: String,
}

/// Per-file scan row. Only files with something to report are listed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixerFileReport {
    /// Mod folder: the first path component under the Mods folder.
    pub folder: String,
    pub ini_path: String,
    /// Primary character, taken from the fix data (upstream's merged database,
    /// so a shared hash can be attributed to a later character module).
    pub character: String,
    /// Every character module that owns a hash found in this file.
    pub characters: Vec<String>,
    pub updated: Vec<HashUpdate>,
    pub additions: Vec<SectionAddition>,
    pub unsupported: Vec<UnsupportedCommand>,
    /// True when at least one fix needs the `.buf` pass v1 does not implement.
    pub needs_buffer_pass: bool,
    /// True when the user told the fixer to leave this file alone. The fixes are
    /// still listed so they stay visible, but `fixer_apply` never writes the
    /// file, and it is not selected by default.
    pub ignored: bool,
    /// What matched the ignore list: `"mod"` when the whole mod folder is
    /// ignored, `"file"` when only this INI is. `None` when not ignored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignored_scope: Option<String>,
    /// Set when the file could not be processed at all; `updated`/`additions`
    /// are then empty because nothing was written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixerScanResult {
    /// `.ini` files examined.
    pub scanned: u32,
    /// `.ini` files ignored by the loader's own rules (`DISABLED*`, `DESKTOP*`).
    pub skipped: u32,
    /// Outdated files, or files that could not be read.
    pub files: Vec<FixerFileReport>,
}

/// One file that was rewritten.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixerApplied {
    pub ini_path: String,
    /// Mod folder the INI belongs to; carried so the history entry can be shown
    /// after the mod itself is gone from a later scan.
    pub folder: String,
    /// Primary character from the fix data, as the scan reports it.
    pub character: String,
    /// Unix seconds of this apply run; with `iniPath` it identifies the history
    /// entry a restore request means.
    pub fixed_at: u64,
    /// Backup that now holds the pre-fix file; `None` when `backup` was false.
    pub backup_path: Option<String>,
    /// Only the changes that were actually written; ignored ones are left out.
    pub updated: Vec<HashUpdate>,
    pub additions: Vec<SectionAddition>,
    pub unsupported: Vec<UnsupportedCommand>,
    pub needs_buffer_pass: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixerFailure {
    pub ini_path: String,
    pub error: String,
}

/// A file left alone because its only remaining fixes are buffer-level.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixerBufferPending {
    pub ini_path: String,
    /// The buffer-pass commands found in the file.
    pub commands: Vec<UnsupportedCommand>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixerApplyResult {
    pub applied: Vec<FixerApplied>,
    pub failed: Vec<FixerFailure>,
    /// Nothing was written for these; `.buf` support is a separate pass.
    pub needs_buffer_pass: Vec<FixerBufferPending>,
    /// Requested files that were already up to date.
    pub unchanged: Vec<String>,
    /// Requested files left untouched because the ignore list covers them.
    pub ignored: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixerRollbackResult {
    /// INI paths put back, in the order they were requested - the shape the
    /// Mod Fixer screen reads.
    pub restored: Vec<String>,
    pub failed: Vec<FixerFailure>,
}

/// One entry the user told the fixer to leave alone, persisted in the config
/// folder so it survives restarts, route changes and later scans.
///
/// Scope is implied by the fields, most specific first: `key` (with `ini_path`)
/// ignores one recommendation inside one INI, `ini_path` alone ignores the whole
/// INI, and both empty ignores the mod folder (every INI under it, now and
/// later).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixerIgnoredEntry {
    /// Mod folder: the first path component under the Mods folder.
    pub folder: String,
    /// Absolute INI path for a file- or recommendation-scoped ignore.
    #[serde(default)]
    pub ini_path: Option<String>,
    /// Recommendation key (`hash:...`/`section:...`) for a per-fix ignore.
    #[serde(default)]
    pub key: Option<String>,
    /// Human label for the UI, e.g. `S11Mod` or `S11Mod\S11.ini · 3c8697e8`.
    pub label: String,
    /// Unix seconds the entry was added.
    pub ignored_at: u64,
}

/// One successful fix, as stored. The backup path is the only thing a restore
/// needs; it is never taken from the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixerHistoryRecord {
    pub ini_path: String,
    pub folder: String,
    pub character: String,
    #[serde(default)]
    pub backup_path: Option<String>,
    pub fixed_at: u64,
    pub updated: u32,
    pub additions: u32,
    /// Set when this fix was undone through the history menu.
    #[serde(default)]
    pub restored_at: Option<u64>,
}

impl From<&FixerApplied> for FixerHistoryRecord {
    fn from(applied: &FixerApplied) -> Self {
        Self {
            ini_path: applied.ini_path.clone(),
            folder: applied.folder.clone(),
            character: applied.character.clone(),
            backup_path: applied.backup_path.clone(),
            fixed_at: applied.fixed_at,
            updated: applied.updated.len() as u32,
            additions: applied.additions.len() as u32,
            restored_at: None,
        }
    }
}

/// A stored fix, plus whether its backup is still on disk *right now* - the
/// only honest answer to "can this be restored?".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixerHistoryEntry {
    pub ini_path: String,
    pub folder: String,
    pub character: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
    /// False when no backup was kept, the file was deleted, or another restore
    /// already consumed it.
    pub backup_available: bool,
    pub fixed_at: u64,
    pub updated: u32,
    pub additions: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored_at: Option<u64>,
}

/// Everything the Mod Fixer screen keeps between visits: the ignore list and
/// the history of successful fixes, newest first.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixerStateView {
    pub ignored: Vec<FixerIgnoredEntry>,
    pub history: Vec<FixerHistoryEntry>,
}
