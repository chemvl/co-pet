//! Click-through for the pet panel. The panel covers the whole work area but only its hit regions
//! take the mouse: a 50 ms loop hit-tests the cursor against the regions the frontend reports and
//! flips `ignoresMouseEvents` on transitions. It also detects OS drags (left button down while the
//! drag pasteboard changed since the last button-up) to grow the pet region into the drop halo.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use objc2::MainThreadMarker;
use objc2_app_kit::NSEvent;
use tauri::{AppHandle, Manager};

use super::{pasteboard, screens};
use crate::events::{self, PointerRegion};
use crate::model::HitRegion;
use crate::windows;

const TICK: Duration = Duration::from_millis(50);
/// Monitor-set poll every 2 s.
const SCREEN_POLL_TICKS: u32 = 40;
/// `drop-halo`: how far the pet region grows while something is being dragged.
const HALO: f64 = 48.0;

#[derive(Default)]
pub struct PointerState {
    inner: Mutex<Inner>,
    started: AtomicBool,
    /// A tick is queued on the main thread; never queue a second one behind it.
    queued: AtomicBool,
}

#[derive(Default)]
struct Inner {
    regions: Vec<HitRegion>,
    moving: bool,
    hidden: bool,
    /// Whether the panel currently ignores the mouse (it starts ignoring).
    passthrough: bool,
    last: Option<PointerRegion>,
    button_down: bool,
    /// Drag pasteboard change count at the last left-button-up.
    drag_baseline: Option<isize>,
    ticks: u32,
}

impl PointerState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_regions(&self, regions: Vec<HitRegion>) {
        self.lock().regions = regions;
    }

    /// While moving, the whole window takes the mouse so the drag never falls through.
    pub fn set_moving(&self, moving: bool) {
        self.lock().moving = moving;
    }

    /// While hidden the panel ignores the mouse and region checks stop.
    pub fn set_hidden(&self, hidden: bool) {
        self.lock().hidden = hidden;
    }

    /// Records that the pet window was just created with mouse events ignored.
    pub(crate) fn reset_for_new_window(&self) {
        let mut s = self.lock();
        s.passthrough = true;
        s.last = None;
    }
}

/// Starts the loop once; it idles while no pet window exists.
pub fn start(app: &AppHandle) {
    let state = app.state::<PointerState>();
    if state.started.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let state = app.state::<PointerState>();
            if state.queued.swap(true, Ordering::AcqRel) {
                continue;
            }
            let handle = app.clone();
            let queued = app.run_on_main_thread(move || {
                if let Some(mtm) = MainThreadMarker::new() {
                    tick(&handle, mtm);
                }
                handle.state::<PointerState>().queued.store(false, Ordering::Release);
            });
            if queued.is_err() {
                break; // the event loop is gone: the app is exiting
            }
        }
    });
}

fn tick(app: &AppHandle, mtm: MainThreadMarker) {
    let Some(panel) = windows::pet_panel(app) else { return };
    let window = panel.as_panel();
    let state = app.state::<PointerState>();

    let poll_screens = {
        let mut s = state.lock();
        s.ticks = s.ticks.wrapping_add(1);
        s.ticks % SCREEN_POLL_TICKS == 0
    };
    if poll_screens {
        windows::refresh_screens(app, mtm);
    }

    let mut s = state.lock();
    if s.hidden {
        if !s.passthrough {
            window.setIgnoresMouseEvents(true);
            s.passthrough = true;
        }
        emit_if_changed(app, &mut s, PointerRegion { region: None, dragging: false });
        return;
    }

    let left_down = NSEvent::pressedMouseButtons() & 1 != 0;
    let dragging = if left_down {
        s.button_down = true;
        let baseline = *s.drag_baseline.get_or_insert_with(|| pasteboard::change_count(pasteboard::Board::Drag));
        pasteboard::change_count(pasteboard::Board::Drag) != baseline
    } else {
        if s.button_down || s.drag_baseline.is_none() {
            s.drag_baseline = Some(pasteboard::change_count(pasteboard::Board::Drag));
        }
        s.button_down = false;
        false
    };

    let (mx, my) = screens::mouse_location(mtm);
    let frame = screens::window_frame(mtm, window);
    let region = if screens::contains(&frame, mx, my) {
        hit(&s.regions, mx - frame.x, my - frame.y, dragging)
    } else {
        None
    };

    if s.moving {
        drop(s);
        windows::follow_pointer_screen(app, mtm, mx, my);
        s = state.lock();
    }

    let inside = region.is_some() || s.moving;
    if s.passthrough == inside {
        window.setIgnoresMouseEvents(!inside);
        s.passthrough = !inside;
    }
    emit_if_changed(app, &mut s, PointerRegion { region, dragging });
}

fn emit_if_changed(app: &AppHandle, s: &mut Inner, next: PointerRegion) {
    if s.last.as_ref() == Some(&next) {
        return;
    }
    #[cfg(debug_assertions)]
    eprintln!("[pointer] region={:?} dragging={}", next.region, next.dragging);
    events::emit(app, events::POINTER_REGION, next.clone());
    s.last = Some(next);
}

/// Panels win over the pet; while dragging the pet grows by the halo and reports `halo`.
fn hit(regions: &[HitRegion], x: f64, y: f64, dragging: bool) -> Option<&'static str> {
    let inside = |r: &HitRegion, pad: f64| {
        x >= r.x - pad && x < r.x + r.w + pad && y >= r.y - pad && y < r.y + r.h + pad
    };
    if regions.iter().any(|r| r.kind == "panel" && inside(r, 0.0)) {
        return Some("panel");
    }
    let (pad, name) = if dragging { (HALO, "halo") } else { (0.0, "pet") };
    regions.iter().any(|r| r.kind == "pet" && inside(r, pad)).then_some(name)
}
