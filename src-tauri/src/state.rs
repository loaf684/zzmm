use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

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
    /// Cooperative cancel flag for the install currently accepting one.
    pub install_cancel: Arc<AtomicBool>,
}

impl AppState {
    pub fn new(paths: AppPaths, settings: Settings) -> Self {
        Self {
            paths,
            settings: RwLock::new(settings),
            hotkey_cache: Arc::new(Mutex::new(HashMap::new())),
            install_cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}
