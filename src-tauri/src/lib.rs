mod api;
mod error;
mod ini_parser;
mod installer;
mod mesh_loader;
mod mods;
mod settings;
mod state;
mod texture;
mod util;

use std::path::Path;

use tauri::{Manager, Wry};
use tauri_plugin_log::{Target, TargetKind};

use crate::settings::{commands, paths, store};
use crate::state::AppState;

/// File + console logging. The log directory is the platform app-log directory
/// (on Windows: %LOCALAPPDATA%\<identifier>\logs).
fn logger() -> tauri::plugin::TauriPlugin<Wry> {
    tauri_plugin_log::Builder::new()
        .clear_targets()
        .target(Target::new(TargetKind::LogDir {
            file_name: Some("zzmm".into()),
        }))
        .target(Target::new(TargetKind::Stdout))
        .level(log::LevelFilter::Info)
        .max_file_size(5 * 1024 * 1024)
        .build()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .plugin(logger())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let paths = paths::AppPaths::resolve(app.handle())?;
            store::ensure_config_dir(&paths)?;

            let settings = store::load(&paths.settings_file);

            log::info!(
                "ZZMM {} starting — config: {}, logs: {}",
                app.package_info().version,
                paths.config_dir.display(),
                paths.log_dir.display()
            );
            match &settings.mods_dir {
                Some(dir) => {
                    log::info!("Mods directory configured: {dir}");
                    // Preview images are served through the asset protocol, whose
                    // scope is granted at runtime because the folder is user-chosen.
                    if let Err(error) = mods::asset_scope::allow(app.handle(), Path::new(dir)) {
                        log::warn!("Could not grant thumbnail access to {dir}: {error}");
                    }
                }
                None => log::info!("No mods directory configured yet"),
            }

            app.manage(AppState::new(paths, settings));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::validate_mods_dir,
            commands::choose_mods_folder,
            commands::detect_mods_dirs,
            mods::commands::scan_mods,
            mods::commands::set_mod_enabled,
            mods::commands::apply_mod_states,
            mods::commands::set_mod_character,
            mods::commands::assign_mods_character,
            mods::commands::delete_mod,
            mods::commands::open_mod_folder,
            mods::commands::get_mod_details,
            mods::commands::set_mod_notes,
            mods::commands::rename_mod,
            mods::commands::change_mod_preview,
            mods::commands::open_external_url,
            mods::commands::list_loadouts,
            mods::commands::save_loadout,
            mods::commands::delete_loadout,
            mods::commands::apply_loadout,
            installer::commands::install_archives,
            installer::commands::cancel_install,
            api::commands::browse_gamebanana,
            api::commands::get_gamebanana_mod,
            api::commands::get_gamebanana_categories,
            api::commands::install_gamebanana_mod,
            api::commands::check_mod_updates,
            ini_parser::commands::list_mod_ini_files,
            ini_parser::commands::get_mod_hotkeys,
            ini_parser::commands::read_mod_ini,
            ini_parser::commands::save_mod_ini_raw,
            ini_parser::commands::update_ini_setting,
            ini_parser::commands::apply_mod_hotkey_edits,
            ini_parser::commands::scan_hotkey_conflicts,
            mesh_loader::commands::load_mod_preview,
            mesh_loader::commands::save_preview_toggles,
        ])
        .run(tauri::generate_context!());

    // A failed `run` means the event loop never started (usually a missing
    // WebView2 runtime or a broken window config). With `panic = "abort"` an
    // `expect` here would kill the process silently, so log it and exit with a
    // non-zero code that the installer and shells can see.
    if let Err(error) = result {
        log::error!("ZZMM stopped: {error}");
        std::process::exit(1);
    }
}
