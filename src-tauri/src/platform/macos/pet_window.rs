//! The pet window as an NSPanel: floating, non-activating, on every Space, and ignoring the mouse
//! until the pointer loop says otherwise. Also the overlay title bar of Home and Onboarding, and
//! app activation (Dock and Cmd+Tab presence).
use std::time::Duration;

use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSWindow};
use objc2_foundation::NSObjectProtocol;
use tauri::{
    ActivationPolicy, AppHandle, LogicalPosition, Manager, TitleBarStyle, WebviewWindow, WebviewWindowBuilder, Wry,
};
use tauri_nspanel::{CollectionBehavior, ManagerExt, PanelHandle, PanelLevel, StyleMask, WebviewWindowExt};

use super::{drop_target, screens};
use crate::error::{AppError, AppResult};
use crate::model::Rect;
use crate::platform::{on_main, on_main_sync};

/// Traffic lights centred in the 44 px custom title bar (Home and Onboarding).
const TRAFFIC_LIGHTS: LogicalPosition<f64> = LogicalPosition::new(16.0, 24.0);
/// When the deferred activation runs, and when it is retried if Co-pet still isn't active.
const FRONT_DELAYS: [Duration; 2] = [Duration::from_millis(80), Duration::from_millis(250)];

/// The macro brings its own imports, so it gets a module of its own.
mod pet_panel {
    tauri_nspanel::tauri_panel! {
        panel!(PetPanel {
            config: {
                can_become_key_window: true,
                is_floating_panel: true
            }
        })
    }
}
use pet_panel::PetPanel;

fn panel(w: &WebviewWindow) -> Option<PanelHandle<Wry>> {
    w.app_handle().get_webview_panel(w.label()).ok()
}

/// Registers tauri-nspanel, which `configure` needs to turn the pet window into a panel.
pub fn plugin(b: tauri::Builder<Wry>) -> tauri::Builder<Wry> {
    b.plugin(tauri_nspanel::init())
}

/// Builder options for the pet window.
pub fn builder<'a>(b: WebviewWindowBuilder<'a, Wry, AppHandle>) -> WebviewWindowBuilder<'a, Wry, AppHandle> {
    b.visible_on_all_workspaces(true)
        // The app is never active, so the first click must reach the page.
        .accept_first_mouse(true)
}

/// Turns the new pet window into a floating, non-activating panel on every Space that ignores the
/// mouse, with the native drop target over its webview.
pub fn configure(app: &AppHandle, mtm: MainThreadMarker, w: &WebviewWindow) -> AppResult<()> {
    let panel = w.to_panel::<PetPanel>()?;
    panel.set_level(PanelLevel::Floating.into());
    panel
        .add_style_mask(StyleMask::empty().nonactivating_panel().into())
        .map_err(|e| AppError::msg(format!("pet panel style: {e}")))?;
    panel.set_collection_behavior(
        CollectionBehavior::new().can_join_all_spaces().stationary().ignores_cycle().into(),
    );
    panel.as_panel().setIgnoresMouseEvents(true);
    drop_target::install(app, mtm, panel.as_panel());
    Ok(())
}

/// The pet window's frame in global top-left points.
pub fn frame(mtm: MainThreadMarker, w: &WebviewWindow) -> Rect {
    panel(w).map_or(Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }, |p| screens::window_frame(mtm, p.as_panel()))
}

/// Moves and resizes the pet window to `frame` (global top-left points).
pub fn set_frame(mtm: MainThreadMarker, w: &WebviewWindow, frame: Rect) {
    if let Some(p) = panel(w) {
        screens::set_window_frame(mtm, p.as_panel(), frame);
    }
}

pub fn set_ignores_mouse(_mtm: MainThreadMarker, w: &WebviewWindow, ignore: bool) {
    if let Some(p) = panel(w) {
        p.as_panel().setIgnoresMouseEvents(ignore);
    }
}

/// orderFrontRegardless: visible above other apps without becoming key.
pub fn show(_mtm: MainThreadMarker, w: &WebviewWindow) {
    if let Some(p) = panel(w) {
        p.show();
    }
}

/// Makes the panel key (typing goes to the pet) without activating Co-pet.
pub fn focus(app: &AppHandle, w: &WebviewWindow) -> AppResult<()> {
    let panel = panel(w).ok_or_else(|| AppError::msg("no pet window"))?;
    on_main_sync(app, move |_| panel.show_and_make_key())
}

/// Gives keyboard focus back to the app the user was typing in. `resignKeyWindow` must not be
/// sent directly (AppKit then picks an arbitrary next key window and activates its app); ordering
/// the key panel out returns key to the active app, and ordering it back in keeps it visible.
pub fn resign(app: &AppHandle, w: &WebviewWindow) -> AppResult<()> {
    let panel = panel(w).ok_or_else(|| AppError::msg("no pet window"))?;
    on_main_sync(app, move |_| {
        if panel.as_panel().isKeyWindow() {
            panel.hide();
            panel.show();
        }
    })
}

/// A titled window whose content runs under a transparent title bar: native traffic lights over
/// the page's own 44 px title bar.
pub fn overlay<'a>(b: WebviewWindowBuilder<'a, Wry, AppHandle>) -> WebviewWindowBuilder<'a, Wry, AppHandle> {
    b.title_bar_style(TitleBarStyle::Overlay).hidden_title(true).traffic_light_position(TRAFFIC_LIGHTS)
}

/// Activates Co-pet and brings `window` to front on later main-loop turns. Opened from the
/// menu-bar status menu, the immediate `set_focus` runs while the menu is still tracking and the
/// menu's close leaves the previous app frontmost; activating after the menu has closed sticks.
/// The first try uses the cooperative `activate` (macOS 14+), the retry forces it.
pub fn bring_to_front(window: &WebviewWindow) {
    let window = window.clone();
    tauri::async_runtime::spawn(async move {
        for (attempt, delay) in FRONT_DELAYS.into_iter().enumerate() {
            tokio::time::sleep(delay).await;
            let target = window.clone();
            let app = window.app_handle().clone();
            match on_main(&app, move |mtm| front_now(mtm, &target, attempt)).await {
                Ok(false) => continue,
                Ok(true) => break,
                Err(e) => {
                    eprintln!("[windows] bring to front: {e}");
                    break;
                }
            }
        }
    });
}

/// One activation attempt; true once Co-pet is active with `window` key (or the window is gone).
fn front_now(mtm: MainThreadMarker, window: &WebviewWindow, attempt: usize) -> bool {
    let Ok(ptr) = window.ns_window() else { return true };
    // SAFETY: tauri's NSWindow for a live window, used on the main thread.
    let ns: &NSWindow = unsafe { &*ptr.cast::<NSWindow>() };
    let nsapp = NSApplication::sharedApplication(mtm);
    if nsapp.isActive() && ns.isKeyWindow() {
        return true;
    }
    #[cfg(debug_assertions)]
    eprintln!("[windows] {}: activating (attempt {}, active={})", window.label(), attempt + 1, nsapp.isActive());
    if attempt == 0 && nsapp.respondsToSelector(sel!(activate)) {
        nsapp.activate();
    } else {
        #[allow(deprecated)]
        nsapp.activateIgnoringOtherApps(true);
        ns.orderFrontRegardless();
    }
    ns.makeKeyAndOrderFront(None);
    false
}

/// Shows Co-pet in the Dock and Cmd+Tab (`visible`), or keeps it a menu-bar-only accessory.
pub fn set_in_task_switcher(app: &AppHandle, visible: bool) -> AppResult<()> {
    app.set_activation_policy(if visible { ActivationPolicy::Regular } else { ActivationPolicy::Accessory })?;
    Ok(())
}
