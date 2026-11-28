use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tokio::sync::Mutex as AsyncMutex;

use crate::ini_parser::model::CachedModHotkeys;
use crate::settings::model::Settings;
use crate::settings::paths::AppPaths;

/// State shared by all commands. Settings are cached in memory and mirrored to
/// the JSON file by the settings commands. The hotkey cache is keyed by mod
/// folder and invalidated per INI via path + size + modified time.
pub struct AppState {
    pub paths: AppPaths,
    pub settings: RwLock<Settings>,
    pub hotkey_cache: Arc<Mutex<HashMap<String, CachedModHotkeys>>>,
    /// Cooperative cancel flag for the local archive batch currently accepting
    /// one. GameBanana downloads have their own per-mod flags below.
    pub install_cancel: Arc<AtomicBool>,
    /// Cancel flag per GameBanana mod download. Keeping one flag per mod means
    /// cancelling one download never stops another that is still running.
    pub gamebanana_cancels: Mutex<HashMap<u64, Arc<AtomicBool>>>,
    /// Serializes the install phase of every downloader. Downloads stream in
    /// parallel, but extracting and moving folders into one Mods directory must
    /// not race on destination names or per-character enable decisions.
    pub install_lock: Arc<AsyncMutex<()>>,
}

impl AppState {
    pub fn new(paths: AppPaths, settings: Settings) -> Self {
        Self {
            paths,
            settings: RwLock::new(settings),
            hotkey_cache: Arc::new(Mutex::new(HashMap::new())),
            install_cancel: Arc::new(AtomicBool::new(false)),
            gamebanana_cancels: Mutex::new(HashMap::new()),
            install_lock: Arc::new(AsyncMutex::new(())),
        }
    }
}
