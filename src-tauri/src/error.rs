use serde::ser::{Serialize, SerializeStruct, Serializer};

pub type AppResult<T> = Result<T, AppError>;

/// Typed backend errors. Each is serialized as `{ code, message }`, which the
/// frontend's `ApiError` wrapper turns into a toast-friendly message.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Could not resolve an application directory: {0}")]
    Path(#[from] tauri::Error),

    #[error("File system error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Could not serialize settings: {0}")]
    Json(#[from] serde_json::Error),

    #[error("The Mods folder no longer exists: {0}")]
    ModsDirMissing(String),

    #[error("That path is not a folder: {0}")]
    ModsDirNotDir(String),

    #[error("The folder picker could not be opened: {0}")]
    Dialog(String),

    #[error("A background task failed: {0}")]
    Task(String),

    #[error("Internal settings state is unavailable. Restart ZZMM and try again.")]
    StateLock,

    #[error("No Mods folder is configured. Link one in Settings first.")]
    NoModsDir,

    #[error("Invalid mod folder name: {0}")]
    InvalidModFolder(String),

    #[error("The mod folder no longer exists: {0}")]
    ModFolderMissing(String),

    #[error("A folder with that name already exists: {0}")]
    ModFolderExists(String),

    #[error("That mod path is not a directory: {0}")]
    ModNotDirectory(String),

    #[error("Could not open Explorer: {0}")]
    Explorer(String),

    #[error("The mod's .zzmm.json is not valid JSON, so it was left untouched: {0}")]
    InvalidMetadata(String),

    #[error("Invalid preview image: {0}")]
    InvalidPreview(String),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Invalid character name: {0}")]
    InvalidCharacter(String),

    #[error("Loadout not found: {0}")]
    LoadoutNotFound(String),

    #[error("Invalid loadout name: {0}")]
    InvalidLoadout(String),

    #[error("Could not grant thumbnail access to the Mods folder: {0}")]
    AssetScope(String),

    #[error("Unsupported archive format (use .zip, .7z or .rar): {0}")]
    ArchiveUnsupported(String),

    #[error("Could not extract the archive: {0}")]
    ArchiveExtract(String),

    #[error("Installation failed: {0}")]
    InstallFailed(String),

    #[error("Installation cancelled.")]
    Cancelled,

    #[error("Could not reach GameBanana: {0}")]
    Network(String),

    #[error("GameBanana request failed: {0}")]
    GameBanana(String),

    #[error("Download failed: {0}")]
    Download(String),

    #[error("INI file not found: {0}")]
    IniNotFound(String),

    #[error("Invalid INI path or section: {0}")]
    IniPath(String),

    #[error("No undo snapshot is stored for that mod: {0}")]
    UndoNotFound(String),

    #[error("Could not build a 3D preview: {0}")]
    Preview(String),

    #[error("Could not decode texture: {0}")]
    Texture(String),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let code = match self {
            AppError::Path(_) => "path_error",
            AppError::Io(_) => "io_error",
            AppError::Json(_) => "json_error",
            AppError::ModsDirMissing(_) | AppError::ModsDirNotDir(_) => "invalid_mods_dir",
            AppError::Dialog(_) => "dialog_error",
            AppError::Task(_) => "task_failed",
            AppError::StateLock => "state_locked",
            AppError::NoModsDir => "no_mods_dir",
            AppError::InvalidModFolder(_) => "invalid_mod_folder",
            AppError::ModFolderMissing(_) => "mod_folder_missing",
            AppError::ModFolderExists(_) => "mod_folder_exists",
            AppError::ModNotDirectory(_) => "mod_not_directory",
            AppError::Explorer(_) => "explorer_error",
            AppError::InvalidMetadata(_) => "invalid_metadata",
            AppError::InvalidPreview(_) => "invalid_preview",
            AppError::InvalidUrl(_) => "invalid_url",
            AppError::InvalidCharacter(_) => "invalid_character",
            AppError::LoadoutNotFound(_) => "loadout_not_found",
            AppError::InvalidLoadout(_) => "invalid_loadout",
            AppError::AssetScope(_) => "asset_scope_error",
            AppError::ArchiveUnsupported(_) => "archive_unsupported",
            AppError::ArchiveExtract(_) => "archive_extract",
            AppError::InstallFailed(_) => "install_failed",
            AppError::Cancelled => "install_cancelled",
            AppError::Network(_) => "network_error",
            AppError::GameBanana(_) => "gamebanana_error",
            AppError::Download(_) => "download_error",
            AppError::IniNotFound(_) => "ini_not_found",
            AppError::IniPath(_) => "ini_path_error",
            AppError::UndoNotFound(_) => "undo_not_found",
            AppError::Preview(_) => "preview_error",
            AppError::Texture(_) => "texture_error",
        };

        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("code", code)?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}
