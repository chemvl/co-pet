//! App windows. The pet lives in one transparent, non-activating panel that covers the work area
//! of the monitor it is on; the frontend draws the pet and its panels inside it and reports hit
//! regions so everything else clicks through (see `platform::macos::pointer`).
use std::sync::{Mutex, MutexGuard};

use objc2::MainThreadMarker;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Wry};
use tauri_nspanel::{
    CollectionBehavior, ManagerExt, PanelHandle, PanelLevel, StyleMask, WebviewWindowExt,
};

use crate::db::settings;
use crate::error::{AppError, AppResult};
use crate::events;
use crate::model::Placement;
use crate::platform::macos::pointer::{self, PointerState};
use crate::platform::macos::screens::{self, Screen};
use crate::platform::macos::on_main_sync;

pub const PET: &str = "pet";
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
        // DOM drag events fire only without the native drop handler (Step 6).
        .disable_drag_drop_handler()
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

/// Gives keyboard focus back to the app the user was typing in.
pub fn resign_pet_panel(app: &AppHandle) -> AppResult<()> {
    let panel = pet_panel(app).ok_or_else(|| AppError::msg("no pet window"))?;
    on_main_sync(app, move |_| panel.resign_key_window())
}
