//! Data shapes crossing the IPC bridge for the mod fixer.
//!
//! Field names are fixed by the UI that consumes them (`fixerScan`,
//! `fixerApply`, `fixerRollback`); extra fields may be added, existing ones must
//! keep their meaning.

use serde::Serialize;

/// One `hash = <old>` line rewritten to a newer hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HashUpdate {
    pub from: String,
    pub to: String,
    /// Which fix this came from, e.g. `1.0: Soldier11 HeadA Diffuse 1024p Hash`.
    pub label: String,
}

/// One section added under a different hash (the "multiply"/"add if missing"
/// fixes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionAddition {
    /// Section header as written, `[TextureOverride…]`.
    pub section: String,
    /// Hash the added section keys off.
    pub hash: String,
    pub label: String,
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
    /// Backup that now holds the pre-fix file; `None` when `backup` was false.
    pub backup_path: Option<String>,
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixerRollbackResult {
    /// INI paths put back, in the order they were requested - the shape the
    /// Mod Fixer screen reads.
    pub restored: Vec<String>,
    pub failed: Vec<FixerFailure>,
}
