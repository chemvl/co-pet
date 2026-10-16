mod commands;
mod db;
mod error;
mod events;
mod model;
mod i18n;
mod library;
mod paths;
mod extract;
mod llm;
mod triage;
mod platform;
mod windows;

use tauri::Manager;

use crate::db::Db;
use crate::paths::Paths;

pub fn run() {
    tauri::Builder::default()
        // Registered first so a second launch never gets as far as setup.
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {
            eprintln!("[app] second launch: the running instance stays as it is");
        }))
        .plugin(tauri_nspanel::init())
        .manage(platform::macos::pointer::PointerState::default())
        .manage(windows::PetPlacement::default())
        .setup(|app| {
            // No Dock icon: the pet and the menu bar icon are the entry points.
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let handle = app.handle().clone();
            let paths = Paths::resolve(&handle)?;
            paths.ensure()?;
            let db = Db::open(&paths.db)?;
            paths.clear_tmp()?;
            app.manage(paths);
            app.manage(db);
            app.manage(llm::download::Downloader::default());
            app.manage(llm::LlmServer::default());
            app.state::<llm::download::Downloader>().resume_if_needed(&handle);
            // Onboarding (Step 13) is not built yet, so the pet opens whatever
            // `onboarding.completed` says.
            windows::create_pet_window(&handle)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_setting,
            commands::list_items,
            commands::get_item,
            commands::counts,
            commands::set_hit_regions,
            commands::set_pet_moving,
            commands::commit_pet_position,
            commands::get_placement,
            commands::focus_pet_panel,
            commands::resign_pet_panel,
            commands::request_permission,
            commands::permissions_status,
            commands::set_pinned,
            commands::archive,
            commands::trash,
            commands::undo_event,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Co-pet");
}
