//! App windows. The pet lives in one transparent, non-activating panel that covers the work area
//! of the monitor it is on; the frontend draws the pet and its panels inside it and reports hit
//! regions so everything else clicks through (see `platform::macos::pointer`).
use std::sync::{Mutex, MutexGuard};

use objc2::MainThreadMarker;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{
    ActivationPolicy, AppHandle, Emitter, LogicalPosition, Manager, TitleBarStyle, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent, Wry,
};
use tauri_nspanel::{
    CollectionBehavior, ManagerExt, PanelHandle, PanelLevel, StyleMask, WebviewWindowExt,
};

use crate::db::settings;
use crate::error::{AppError, AppResult};
use crate::events;
use crate::model::{Placement, Rect};
use crate::platform::macos::pointer::{self, PointerState};
use crate::platform::macos::screens::{self, Screen};
use crate::platform::macos::on_main_sync;

pub const PET: &str = "pet";
pub const HOME: &str = "home";
pub const ONBOARDING: &str = "onboarding";
/// Traffic lights centred in the 44 px custom title bar (Home and Onboarding).
const TRAFFIC_LIGHTS: LogicalPosition<f64> = LogicalPosition::new(16.0, 24.0);
const POSITIONS: &str = "pet.positions";
const EDGES: [&str; 3] = ["left", "right", "bottom"];
const DEFAULT_EDGE: &str = "right";
const DEFAULT_OFFSET: f64 = 0.6;

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

/// Where the pet is now. Written on the main thread; read by commands.
#[derive(Default)]
pub struct PetPlacement(Mutex<Option<Current>>);

impl PetPlacement {
    fn lock(&self) -> MutexGuard<'_, Option<Current>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Debug, Clone)]
struct Current {
    placement: Placement,
    screen: Screen,
    setup_key: String,
}

/// One `pet.positions[setupKey]` entry.
#[derive(Debug, Serialize, Deserialize)]
struct Saved {
    /// `Screen::id` of the monitor the pet was on.
    screen: String,
    edge: String,
    offset: f64,
}

pub fn pet_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(PET)
}

pub(crate) fn pet_panel(app: &AppHandle) -> Option<PanelHandle<Wry>> {
    app.get_webview_panel(PET).ok()
}

/// Creates the pet panel (once) on the pet's monitor and starts the pointer loop.
pub fn create_pet_window(app: &AppHandle) -> AppResult<()> {
    if pet_window(app).is_some() {
        return Ok(());
    }
    let handle = app.clone();
    on_main_sync(app, move |mtm| build_pet_window(&handle, mtm))?
}

fn build_pet_window(app: &AppHandle, mtm: MainThreadMarker) -> AppResult<()> {
    let window = WebviewWindowBuilder::new(app, PET, WebviewUrl::App("pet.html".into()))
        .title("Co-pet")
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .resizable(false)
        .focused(false)
        // The app is never active, so the first click must reach the page.
        .accept_first_mouse(true)
        .visible(false)
        // Drops go to the native drop target (platform::macos::drop_target), never to the page.
        .disable_drag_drop_handler()
        .on_navigation(is_app_url)
        .build()?;

    let panel = window.to_panel::<PetPanel>()?;
    panel.set_level(PanelLevel::Floating.into());
    panel
        .add_style_mask(StyleMask::empty().nonactivating_panel().into())
        .map_err(|e| AppError::msg(format!("pet panel style: {e}")))?;
    panel.set_collection_behavior(
        CollectionBehavior::new().can_join_all_spaces().stationary().ignores_cycle().into(),
    );
    panel.as_panel().setIgnoresMouseEvents(true);
    crate::platform::macos::drop_target::install(app, mtm, panel.as_panel());
    app.state::<PointerState>().reset_for_new_window();

    let current = resolve(app, &screens::screens(mtm))?;
    screens::set_window_frame(mtm, panel.as_panel(), current.screen.visible);
    eprintln!(
        "[pet] placed on {} ({}), work area {:?}, {} {:.2}",
        current.screen.name,
        current.screen.id,
        current.screen.visible,
        current.placement.edge,
        current.placement.offset
    );
    *app.state::<PetPlacement>().lock() = Some(current);
    // orderFrontRegardless: visible above other apps without becoming key.
    panel.show();
    pointer::start(app);
    Ok(())
}

fn placement_for(screen: &Screen, edge: String, offset: f64) -> Placement {
    Placement {
        monitor_name: screen.name.clone(),
        edge,
        offset,
        scale: screen.scale,
        work_area: screen.visible,
    }
}

/// The saved position for this monitor setup, else the primary monitor's right edge at 0.6.
fn resolve(app: &AppHandle, all: &[Screen]) -> AppResult<Current> {
    let primary = all.first().ok_or_else(|| AppError::msg("no display attached"))?;
    let setup_key = screens::setup_key(all);
    let mut positions: Map<String, Value> = settings::read(app, POSITIONS)?;
    let saved = positions
        .remove(&setup_key)
        .and_then(|v| serde_json::from_value::<Saved>(v).ok())
        .filter(|s| EDGES.contains(&s.edge.as_str()) && (0.0..=1.0).contains(&s.offset))
        .and_then(|s| all.iter().find(|x| x.id == s.screen).map(|x| (x, s.edge, s.offset)));
    let (screen, edge, offset) = saved.unwrap_or((primary, DEFAULT_EDGE.to_string(), DEFAULT_OFFSET));
    Ok(Current { placement: placement_for(screen, edge, offset), screen: screen.clone(), setup_key })
}

fn apply(app: &AppHandle, mtm: MainThreadMarker, panel: &PanelHandle<Wry>, next: Current) {
    screens::set_window_frame(mtm, panel.as_panel(), next.screen.visible);
    events::pet_placement(app, next.placement.clone());
    *app.state::<PetPlacement>().lock() = Some(next);
}

/// Pointer-loop poll: a changed monitor set re-resolves the placement; on the same setup the pet
/// follows its monitor's work area (Dock or menu bar changes).
pub(crate) fn refresh_screens(app: &AppHandle, mtm: MainThreadMarker) {
    let Some(panel) = pet_panel(app) else { return };
    let all = screens::screens(mtm);
    let key = screens::setup_key(&all);
    let same_setup = {
        let state = app.state::<PetPlacement>();
        let cur = state.lock();
        match cur.as_ref() {
            Some(c) if c.setup_key == key => match all.iter().find(|s| s.id == c.screen.id) {
                Some(s) if s.visible != c.screen.visible || s.scale != c.screen.scale => Some(Current {
                    placement: placement_for(s, c.placement.edge.clone(), c.placement.offset),
                    screen: s.clone(),
                    setup_key: key.clone(),
                }),
                _ => return,
            },
            _ => None,
        }
    };
    let next = match same_setup {
        Some(next) => next,
        None => match resolve(app, &all) {
            Ok(next) => next,
            Err(e) => {
                eprintln!("[pet] re-place skipped: {e}");
                return;
            }
        },
    };
    eprintln!("[pet] monitors changed; now on {} {:?}", next.screen.id, next.screen.visible);
    apply(app, mtm, &panel, next);
}

/// While drag-moving, the window jumps to the monitor under the cursor (edge and offset kept
/// until the frontend commits).
pub(crate) fn follow_pointer_screen(app: &AppHandle, mtm: MainThreadMarker, x: f64, y: f64) {
    let Some(panel) = pet_panel(app) else { return };
    let all = screens::screens(mtm);
    let Some(target) = all.iter().find(|s| screens::contains(&s.frame, x, y)) else { return };
    let next = {
        let state = app.state::<PetPlacement>();
        let cur = state.lock();
        let Some(c) = cur.as_ref() else { return };
        if c.screen.id == target.id {
            return;
        }
        Current {
            placement: placement_for(target, c.placement.edge.clone(), c.placement.offset),
            screen: target.clone(),
            setup_key: c.setup_key.clone(),
        }
    };
    apply(app, mtm, &panel, next);
}

pub fn get_placement(app: &AppHandle) -> AppResult<Placement> {
    app.state::<PetPlacement>()
        .lock()
        .as_ref()
        .map(|c| c.placement.clone())
        .ok_or_else(|| AppError::msg("pet window not placed"))
}

/// The full frame (global top-left points) of the monitor the pet lives on.
pub(crate) fn pet_screen_frame(app: &AppHandle) -> Option<Rect> {
    pet_window(app)?;
    app.state::<PetPlacement>().lock().as_ref().map(|c| c.screen.frame)
}

/// Saves the snapped position under the current monitor setup and emits `pet-placement`.
pub fn commit_pet_position(app: &AppHandle, edge: &str, offset: f64) -> AppResult<()> {
    if !EDGES.contains(&edge) || !offset.is_finite() {
        return Err(AppError::Code("invalid"));
    }
    let offset = offset.clamp(0.0, 1.0);
    let (key, saved, placement) = {
        let state = app.state::<PetPlacement>();
        let mut cur = state.lock();
        let c = cur.as_mut().ok_or_else(|| AppError::msg("pet window not placed"))?;
        c.placement.edge = edge.to_string();
        c.placement.offset = offset;
        let saved = Saved { screen: c.screen.id.clone(), edge: edge.to_string(), offset };
        (c.setup_key.clone(), saved, c.placement.clone())
    };
    let mut positions: Map<String, Value> = settings::read(app, POSITIONS)?;
    positions.insert(key, serde_json::to_value(saved)?);
    settings::set(app, POSITIONS, &Value::Object(positions))?;
    events::pet_placement(app, placement);
    Ok(())
}

/// Makes the panel key (typing goes to the pet) without activating Co-pet.
pub fn focus_pet_panel(app: &AppHandle) -> AppResult<()> {
    let panel = pet_panel(app).ok_or_else(|| AppError::msg("no pet window"))?;
    on_main_sync(app, move |_| panel.show_and_make_key())
}

/// Gives keyboard focus back to the app the user was typing in. `resignKeyWindow` must not be
/// sent directly (AppKit then picks an arbitrary next key window and activates its app); ordering
/// the key panel out returns key to the active app, and ordering it back in keeps it visible.
pub fn resign_pet_panel(app: &AppHandle) -> AppResult<()> {
    let panel = pet_panel(app).ok_or_else(|| AppError::msg("no pet window"))?;
    on_main_sync(app, move |_| {
        if panel.as_panel().isKeyWindow() {
            panel.hide();
            panel.show();
        }
    })
}

/// Opens Home at `route` (e.g. `settings/general`; default the inbox), or focuses it and emits
/// `navigate`. Co-pet shows in the Dock and Cmd+Tab while Home or Onboarding is open.
pub fn open_home(app: &AppHandle, route: Option<String>) -> AppResult<()> {
    let route = route
        .map(|r| r.trim_start_matches(['#', '/']).to_string())
        .filter(|r| !r.is_empty());
    app.set_activation_policy(ActivationPolicy::Regular)?;
    if let Some(window) = app.get_webview_window(HOME) {
        window.unminimize()?;
        window.show()?;
        window.set_focus()?;
        if let Some(route) = route {
            app.emit_to(HOME, events::NAVIGATE, events::Navigate { route })?;
        }
        return Ok(());
    }
    let url = format!("home.html#/{}", route.as_deref().unwrap_or("inbox"));
    let window = overlay_window(app, HOME, &url)
        .inner_size(1280.0, 800.0)
        .min_inner_size(960.0, 640.0)
        .build()?;
    track_activation_policy(&window);
    window.set_focus()?;
    Ok(())
}

/// Opens (or focuses) the onboarding window.
pub fn open_onboarding(app: &AppHandle) -> AppResult<()> {
    app.set_activation_policy(ActivationPolicy::Regular)?;
    if let Some(window) = app.get_webview_window(ONBOARDING) {
        window.unminimize()?;
        window.show()?;
        return Ok(window.set_focus()?);
    }
    let window = overlay_window(app, ONBOARDING, "onboarding.html")
        .inner_size(960.0, 640.0)
        .resizable(false)
        .maximizable(false)
        .build()?;
    track_activation_policy(&window);
    window.set_focus()?;
    Ok(())
}

/// A titled window whose content runs under a transparent title bar: native traffic lights over
/// the page's own 44 px title bar.
fn overlay_window<'a>(app: &'a AppHandle, label: &str, url: &str) -> WebviewWindowBuilder<'a, Wry, AppHandle> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App(url.into()))
        .on_navigation(is_app_url)
        .title("Co-pet")
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(TRAFFIC_LIGHTS)
        .center()
}

/// When the last of Home/Onboarding closes, Co-pet leaves the Dock again.
fn track_activation_policy(window: &WebviewWindow) {
    let app = window.app_handle().clone();
    let other = if window.label() == HOME { ONBOARDING } else { HOME };
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) && app.get_webview_window(other).is_none() {
            if let Err(e) = app.set_activation_policy(ActivationPolicy::Accessory) {
                eprintln!("[windows] activation policy: {e}");
            }
        }
    });
}

/// Webviews only ever show the app's own pages: a stray file drop or link must not replace them
/// (WebKit's default drop navigates to the dropped file).
fn is_app_url(url: &tauri::Url) -> bool {
    let allowed = match url.scheme() {
        "tauri" => true,
        "http" | "https" => matches!(url.host_str(), Some("localhost" | "tauri.localhost" | "127.0.0.1")),
        _ => false,
    };
    if !allowed {
        eprintln!("[windows] blocked navigation to {url}");
    }
    allowed
}
