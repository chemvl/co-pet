//! Menu bar icon (Step 10.1): the cat-head template image with Open home · Show pet · Pause/Resume
//! pet · Settings · Quit Co-pet. Labels follow `ui.language`; the menu is rebuilt when the
//! language or `pet.paused` changes.
use serde_json::Value;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Listener, Wry};

use crate::db::settings;
use crate::error::AppResult;
use crate::events;
use crate::i18n::{self, tr};
use crate::platform::macos::fullscreen;
use crate::{system, windows};

const TRAY_ID: &str = "copet";
const ICON: &[u8] = include_bytes!("../icons/tray-template.png");

pub fn create(app: &AppHandle) -> AppResult<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(ICON)?)
        .icon_as_template(true)
        .tooltip("Co-pet")
        .menu(&build_menu(app)?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .build(app)?;

    let handle = app.clone();
    app.listen_any(events::SETTINGS_CHANGED, move |event| {
        let payload = serde_json::from_str::<Value>(event.payload()).ok();
        let key = payload.as_ref().and_then(|v| v["key"].as_str());
        if matches!(key, Some("ui.language" | "pet.paused")) {
            if let Err(e) = refresh(&handle) {
                eprintln!("[tray] menu rebuild: {e}");
            }
        }
    });
    Ok(())
}

fn refresh(app: &AppHandle) -> AppResult<()> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(build_menu(app)?))?;
    }
    Ok(())
}

fn build_menu(app: &AppHandle) -> AppResult<Menu<Wry>> {
    let lang = i18n::lang(app);
    let paused = settings::read::<bool>(app, "pet.paused").unwrap_or(false);
    let item = |id: &str, key: &str| MenuItem::with_id(app, id, tr(lang, key), true, None::<&str>);
    let pause = if paused { "tray.resumePet" } else { "tray.pausePet" };
    Ok(Menu::with_items(
        app,
        &[
            &item("openHome", "tray.openHome")?,
            &item("showPet", "tray.showPet")?,
            &item("togglePause", pause)?,
            &PredefinedMenuItem::separator(app)?,
            &item("settings", "tray.settings")?,
            &PredefinedMenuItem::separator(app)?,
            &item("quit", "tray.quit")?,
        ],
    )?)
}

fn on_menu(app: &AppHandle, id: &str) {
    let result = match id {
        "openHome" => windows::open_home(app, None),
        "showPet" => {
            fullscreen::show_now(app);
            Ok(())
        }
        "togglePause" => {
            let paused = settings::read::<bool>(app, "pet.paused").unwrap_or(false);
            settings::set(app, "pet.paused", &Value::Bool(!paused))
        }
        "settings" => windows::open_home(app, Some("settings/general".into())),
        "quit" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { system::quit(&app).await });
            Ok(())
        }
        _ => Ok(()),
    };
    if let Err(e) = result {
        eprintln!("[tray] {id}: {e}");
    }
}
