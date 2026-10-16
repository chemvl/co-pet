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
mod capture;
mod drag;
mod data;
mod hotkeys;
mod system;
mod tray_icon;

use tauri::Manager;

use crate::db::Db;
use crate::paths::Paths;

pub fn run() {
    tauri::Builder::default()
        // Registered first so a second launch never gets as far as setup.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            let opened = if settings_bool(app, "onboarding.completed") {
                windows::open_home(app, None)
            } else {
                windows::open_onboarding(app)
            };
            if let Err(e) = opened {
                eprintln!("[app] second launch: {e}");
            }
        }))
        .plugin(tauri_nspanel::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .manage(hotkeys::Hotkeys::default())
        .manage(platform::macos::fullscreen::Visibility::default())
        .manage(platform::macos::idle::Sleep::default())
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
            // After Paths, Db, Downloader and LlmServer: the worker uses all of them.
            app.manage(triage::start(&handle));
            tray_icon::create(&handle)?;
            hotkeys::init(&handle);
            platform::macos::fullscreen::start(&handle);
            platform::macos::idle::start(&handle);
            system::watch_autostart(&handle);
            if settings_bool(&handle, "onboarding.completed") {
                windows::create_pet_window(&handle)?;
                system::apply_autostart(&handle);
            } else {
                windows::open_onboarding(&handle)?;
            }
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
            commands::capture_drop,
            commands::capture_clipboard,
            commands::capture_screenshot,
            commands::capture_selection,
            commands::capture_note,
            commands::prepare_drag,
            commands::copy_items,
            commands::paste_items,
            commands::ai_status,
            commands::ai_download,
            commands::ai_pause,
            commands::ai_select_model,
            commands::recategorize,
            commands::make_rule_from_item,
            commands::set_pinned,
            commands::archive,
            commands::trash,
            commands::undo_event,
            commands::set_hotkey,
            commands::quit_app,
            commands::pet_status,
            commands::open_home,
            commands::pick_excluded_app,
            commands::list_excluded_apps,
            commands::export_all,
            commands::delete_everything,
            commands::finish_onboarding,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Co-pet")
        .run(system::on_run_event);
}

fn settings_bool(app: &tauri::AppHandle, key: &str) -> bool {
    db::settings::read::<bool>(app, key).unwrap_or(false)
}
