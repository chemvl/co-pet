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
mod ai;
mod triage;
mod next;
mod platform;
mod windows;
mod capture;
mod drag;
mod data;
mod hotkeys;
mod system;
mod tray_icon;
mod secrets;
mod backend;
mod links;
mod export;
mod agent;
mod account;
mod sync;
mod storage;
pub mod transcribe;

use tauri::Manager;
use tauri_plugin_deep_link::DeepLinkExt;

use crate::db::Db;
use crate::error::AppError;
use crate::paths::Paths;

pub fn run() {
    #[cfg(windows)]
    platform::init_main_thread();
    let builder = tauri::Builder::default()
        // Registered first so a second launch never gets as far as setup.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // Windows launches a second instance with the link as an argument; the deep-link
            // plugin forwards it to `on_open_url`, so only a plain relaunch opens a window.
            if argv.iter().any(|a| a.starts_with("copet://")) {
                return;
            }
            let opened = if settings_bool(app, "onboarding.completed") {
                windows::open_home(app, None)
            } else {
                windows::open_onboarding(app)
            };
            if let Err(e) = opened {
                eprintln!("[app] second launch: {e}");
            }
        }));
    platform::pet_window::plugin(builder)
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .manage(hotkeys::Hotkeys::default())
        .manage(platform::fullscreen::Visibility::default())
        .manage(platform::idle::Sleep::default())
        .manage(platform::pointer::PointerState::default())
        .manage(windows::PetPlacement::default())
        .manage(export::oauth::OAuthPending::default())
        .manage(account::auth::GooglePending::default())
        .manage(account::keys::PendingKey::default())
        .setup(|app| {
            // Not in the task switcher: the pet and the menu bar icon are the entry points.
            platform::pet_window::set_in_task_switcher(app.handle(), false)?;
            let handle = app.handle().clone();
            let base = handle.path().app_data_dir()?;
            let default_root = storage::default_root(&handle, &base)?;
            let paths = match storage::startup(&base, &default_root) {
                Ok(loc) => {
                    let mut paths = Paths::new(loc.data_dir.clone().unwrap_or_else(|| default_root.clone()));
                    paths.base = base.clone();
                    paths.set_models(loc.models_dir.clone().unwrap_or_else(|| default_root.join("models")));
                    paths.default_root = default_root;
                    paths
                }
                Err(AppError::Code("dataFolderMissing")) => {
                    let missing = storage::load(&base).data_dir.unwrap_or_default();
                    storage::show_missing_and_quit(&handle, &missing);
                    // `system::on_run_event` only uses `try_state`, so nothing below is needed.
                    return Ok(());
                }
                Err(e) => return Err(e.into()),
            };
            paths.ensure()?;
            let db = Db::open(&paths.db)?;
            paths.clear_tmp()?;
            app.manage(paths);
            // Before any window opens: a data folder outside `$APPDATA` needs runtime scopes.
            storage::grant_scopes(&handle, &app.state::<Paths>())?;
            app.manage(db);
            app.manage(ai::KeyCache::default());
            app.manage(llm::download::Downloader::default());
            app.manage(llm::LlmServer::default());
            #[cfg(target_os = "macos")]
            llm::reap_orphans(&handle);
            system::quit_on_signals(&handle);
            app.state::<llm::download::Downloader>().resume_if_needed(&handle);
            app.manage(transcribe::models::PackDownloader::default());
            app.state::<transcribe::models::PackDownloader>().resume_if_needed(&handle);
            // After Paths, Db, Downloader and LlmServer: the worker uses all of them.
            app.manage(triage::start(&handle));
            tray_icon::create(&handle)?;
            hotkeys::init(&handle);
            platform::fullscreen::start(&handle);
            platform::idle::start(&handle);
            next::start(&handle);
            export::queue::start(&handle);
            export::status::start(&handle);
            // After the export queue: `export_task` pokes it.
            agent::start(&handle);
            sync::start(&handle);
            // The queue before the recorder, which holds it while recording; both before
            // `transcribe::start` (crash recovery and the worker).
            app.manage(transcribe::worker::TranscribeQueue::default());
            app.manage(transcribe::recorder::Recorder::default());
            transcribe::start(&handle);
            system::watch_autostart(&handle);
            // Before any window opens, so none starts in the wrong appearance.
            system::follow_theme(&handle);
            if settings_bool(&handle, "onboarding.completed") {
                windows::create_pet_window(&handle)?;
                system::apply_autostart(&handle);
            } else {
                windows::open_onboarding(&handle)?;
            }
            // The MSIX package registers `copet://` through its manifest; dev builds register at runtime.
            #[cfg(all(windows, debug_assertions))]
            if let Err(e) = app.deep_link().register_all() {
                eprintln!("[links] register: {e}");
            }
            // `copet://` links: while running, and the one that launched the app.
            let links_handle = handle.clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    links::handle(&links_handle, url);
                }
            });
            match app.deep_link().get_current() {
                Ok(urls) => urls.into_iter().flatten().for_each(|url| links::handle(&handle, url)),
                Err(e) => eprintln!("[links] launch link: {e}"),
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
            commands::list_calendars,
            commands::capture_clipboard,
            commands::capture_screenshot,
            commands::capture_selection,
            commands::capture_note,
            commands::prepare_drag,
            #[cfg(windows)]
            commands::start_drag_out,
            commands::copy_items,
            commands::paste_items,
            commands::ai_status,
            commands::ai_download,
            commands::ai_pause,
            commands::ai_select_model,
            commands::ai_configure,
            commands::ai_set_key,
            commands::ai_remove_key,
            commands::ai_test,
            commands::ai_server_models,
            commands::speech_status,
            commands::speech_download,
            commands::speech_pause,
            commands::recording_status,
            commands::start_recording,
            commands::pause_recording,
            commands::resume_recording,
            commands::stop_recording,
            commands::set_recording_language,
            commands::call_detection_status,
            commands::call_keep_recording,
            commands::import_meeting,
            commands::transcribe_item,
            commands::list_meetings,
            commands::get_meeting,
            commands::rename_meeting,
            commands::rename_speaker,
            commands::retry_meeting,
            commands::retranscribe_meeting,
            commands::delete_meeting,
            commands::resummarize_meeting,
            commands::summarize_meeting,
            commands::apply_meeting_change,
            commands::undo_meeting_change,
            commands::ignore_meeting_change,
            commands::add_action_to_tasks,
            commands::copy_meeting_markdown,
            commands::recategorize,
            commands::make_rule_from_item,
            commands::set_pinned,
            commands::edit_item,
            commands::archive,
            commands::trash,
            commands::undo_event,
            commands::set_hotkey,
            commands::suspend_hotkeys,
            commands::quit_app,
            commands::pet_status,
            commands::open_home,
            commands::pick_excluded_app,
            commands::list_excluded_apps,
            commands::export_all,
            commands::delete_everything,
            commands::finish_onboarding,
            commands::list_categories,
            commands::create_category,
            commands::update_category,
            commands::delete_category,
            commands::reorder_categories,
            commands::list_rules,
            commands::add_rule,
            commands::delete_rule,
            commands::restore_rule,
            commands::list_examples,
            commands::delete_example,
            commands::list_tasks,
            commands::get_task,
            commands::create_task,
            commands::update_task,
            commands::set_task_done,
            commands::snooze_task,
            commands::trash_task,
            commands::undo_task_event,
            commands::whats_next,
            commands::next_action,
            commands::backend_status,
            commands::set_backend,
            commands::clear_backend,
            commands::integrations_status,
            commands::connect_oauth,
            commands::connect_token,
            commands::disconnect,
            commands::notion_data_sources,
            commands::notion_schema,
            commands::asana_options,
            commands::set_export_target,
            commands::export_task,
            commands::retry_exports,
            commands::save_tasks_file,
            commands::mcp_status,
            commands::set_mcp_paused,
            commands::list_mcp_clients,
            commands::add_mcp_client,
            commands::approve_mcp_client,
            commands::revoke_mcp_client,
            commands::remove_mcp_client,
            commands::create_http_client,
            commands::list_agent_activity,
            commands::undo_agent_activity,
            commands::account_status,
            commands::send_email_code,
            commands::verify_email_code,
            commands::sign_in_google,
            commands::create_recovery_key,
            commands::confirm_recovery_key,
            commands::enter_recovery_key,
            commands::sign_out,
            commands::delete_cloud_data,
            commands::save_text_file,
            commands::copy_concealed_text,
            commands::sync_status,
            commands::sync_now,
            commands::diary_week,
            commands::storage_locations,
            commands::set_models_dir,
            commands::set_data_dir,
            commands::cancel_data_move,
            commands::restart_app,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Co-pet")
        .run(system::on_run_event);
}

fn settings_bool(app: &tauri::AppHandle, key: &str) -> bool {
    db::settings::read::<bool>(app, key).unwrap_or(false)
}
