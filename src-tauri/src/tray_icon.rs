//! Menu bar icon (Step 10.1): the cat-head template image with (Restart to update to X when an
//! update is ready) · Open home · Show pet · Pause/Resume
//! pet · Settings · Quit Co-pet. Labels follow `ui.language`; the menu is rebuilt when the
//! language or `pet.paused` changes.
use serde_json::Value;
use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Listener, Manager, Wry};

use crate::db::settings;
use crate::error::AppResult;
use crate::events;
use crate::i18n::{self, tr};
use crate::platform::macos::fullscreen;
use crate::model::UpdateState;
use crate::{system, updater, windows};

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

pub(crate) fn refresh(app: &AppHandle) -> AppResult<()> {
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
    let ready = app.try_state::<updater::Updates>().map(|u| u.status()).and_then(|s| match s.state {
        UpdateState::Ready => s.version,
        _ => None,
    });
    let restart = match ready {
        Some(v) => Some((
            MenuItem::with_id(
                app,
                "restartUpdate",
                tr(lang, "tray.restartToUpdate").replace("{version}", &v),
                true,
                None::<&str>,
            )?,
            PredefinedMenuItem::separator(app)?,
        )),
        None => None,
    };
    let rest = [
        item("openHome", "tray.openHome")?,
        item("showPet", "tray.showPet")?,
        item("togglePause", pause)?,
        item("settings", "tray.settings")?,
        item("quit", "tray.quit")?,
    ];
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let mut items: Vec<&dyn IsMenuItem<Wry>> = Vec::with_capacity(9);
    if let Some((restart, sep)) = &restart {
        items.push(restart);
        items.push(sep);
    }
    items.extend([&rest[0] as &dyn IsMenuItem<Wry>, &rest[1], &rest[2], &sep1, &rest[3], &sep2, &rest[4]]);
    Ok(Menu::with_items(app, &items)?)
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
        "restartUpdate" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = updater::install(&app).await {
                    eprintln!("[tray] restartUpdate: {e}");
                }
            });
            Ok(())
        }
        "quit" => {
            system::request_quit(app);
            Ok(())
        }
        _ => Ok(()),
    };
    if let Err(e) = result {
        eprintln!("[tray] {id}: {e}");
    }
}
