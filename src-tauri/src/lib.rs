mod commands;
mod db;
mod error;
mod events;
mod model;
mod paths;

use tauri::Manager;

use crate::db::Db;
use crate::paths::Paths;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let paths = Paths::resolve(&handle)?;
            paths.ensure()?;
            let db = Db::open(&paths.db)?;
            paths.clear_tmp()?;
            app.manage(paths);
            app.manage(db);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_setting,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Co-pet");
}
