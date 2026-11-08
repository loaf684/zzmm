use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    /// Display name of the archive being processed.
    pub archive: String,
    /// One of `extracting`, `detecting`, `installing`, `done`, `failed`,
    /// `cancelled`.
    pub stage: String,
    pub percent: u8,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveInstallOutcome {
    pub archive: String,
    /// Folder names installed into the Mods directory.
    pub installed: Vec<String>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    /// True when the user stopped this archive before it finished.
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallBatchResult {
    pub outcomes: Vec<ArchiveInstallOutcome>,
}
