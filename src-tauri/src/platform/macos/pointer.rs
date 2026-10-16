//! Click-through for the pet panel. The panel covers the whole work area but only its hit regions
//! take the mouse: a 50 ms loop hit-tests the cursor against the regions the frontend reports and
//! flips `ignoresMouseEvents` on transitions. It also detects OS drags (left button down while the
//! drag pasteboard changed since the last button-up) so the pet region grows into the drop halo,
//! and keeps the native drop target's catcher panel over the pet and its halo.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2_app_kit::NSEvent;
use tauri::{AppHandle, Manager};

use super::{drop_target, pasteboard, screens};
use crate::events::{self, PointerRegion};
use crate::model::{HitRegion, Rect};
use crate::windows;

const TICK: Duration = Duration::from_millis(50);
const SCREEN_POLL: Duration = Duration::from_secs(2);
/// `drop-halo`: how far the pet region grows while something is being dragged; the drop
/// target accepts within it.
const HALO: f64 = 48.0;

#[derive(Default)]
pub struct PointerState {
    inner: Mutex<Inner>,
    started: AtomicBool,
    queued: AtomicBool,
}

#[derive(Default)]
struct Inner {
    regions: Vec<HitRegion>,
    moving: bool,
    hidden: bool,
    /// Hidden and the hidden-state housekeeping is done: the loop skips its main-thread hop
    /// until the pet shows again.
    parked: bool,
    /// Whether the panel currently ignores the mouse (it starts ignoring).
    passthrough: bool,
    last: Option<PointerRegion>,
    button_down: bool,
    /// Drag pasteboard change count at the last left-button-up.
    drag_baseline: Option<isize>,
    last_screen_poll: Option<Instant>,
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
        let mut s = self.lock();
        s.hidden = hidden;
        s.parked = false;
    }

    /// Records that the pet window was just created with mouse events ignored.
    pub(crate) fn reset_for_new_window(&self) {
        let mut s = self.lock();
        s.passthrough = true;
        s.parked = false;
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
            if state.lock().parked || state.queued.swap(true, Ordering::AcqRel) {
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
        let due = s.last_screen_poll.is_none_or(|t| t.elapsed() >= SCREEN_POLL);
        if due {
            s.last_screen_poll = Some(Instant::now());
        }
        due
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
        s.parked = true;
        drop(s);
        drop_target::place_catcher(mtm, window, None);
        drop_target::set_dragging(mtm, false, windows_closed(app));
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
    let local = screens::contains(&frame, mx, my).then(|| (mx - frame.x, my - frame.y));
    let region = local.and_then(|(x, y)| hit(&s.regions, x, y, dragging));

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
    // The catcher covers pet + halo in global points; it steps aside while the pet is moved.
    let catcher = if s.moving { None } else { pet_bounds(&s.regions, HALO) };
    drop(s);
    let catcher = catcher.map(|r| Rect { x: r.x + frame.x, y: r.y + frame.y, ..r });
    drop_target::place_catcher(mtm, window, catcher);
    drop_target::set_dragging(mtm, dragging, windows_closed(app));
}

/// Neither Home nor Onboarding is open: Co-pet has no reason to be the active app.
fn windows_closed(app: &AppHandle) -> bool {
    app.get_webview_window(windows::HOME).is_none() && app.get_webview_window(windows::ONBOARDING).is_none()
}

/// Whether a global top-left point is within the drop halo of the pet.
pub(crate) fn in_drop_halo(app: &AppHandle, mtm: MainThreadMarker, x: f64, y: f64) -> bool {
    let Some(panel) = windows::pet_panel(app) else { return false };
    let frame = screens::window_frame(mtm, panel.as_panel());
    let state = app.state::<PointerState>();
    let s = state.lock();
    !s.hidden && near_pet(&s.regions, x - frame.x, y - frame.y, HALO)
}

/// The union of the pet regions grown by `pad` (window CSS px).
fn pet_bounds(regions: &[HitRegion], pad: f64) -> Option<Rect> {
    let (x0, y0, x1, y1) = regions.iter().filter(|r| r.kind == "pet").fold(None, |acc, r| {
        let (a, b, c, d) = (r.x - pad, r.y - pad, r.x + r.w + pad, r.y + r.h + pad);
        Some(match acc {
            None => (a, b, c, d),
            Some((x0, y0, x1, y1)) => (f64::min(x0, a), f64::min(y0, b), f64::max(x1, c), f64::max(y1, d)),
        })
    })?;
    Some(Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
}

fn near_pet(regions: &[HitRegion], x: f64, y: f64, pad: f64) -> bool {
    regions.iter().any(|r| r.kind == "pet" && inside(r, x, y, pad))
}

fn inside(r: &HitRegion, x: f64, y: f64, pad: f64) -> bool {
    x >= r.x - pad && x < r.x + r.w + pad && y >= r.y - pad && y < r.y + r.h + pad
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
    if regions.iter().any(|r| r.kind == "panel" && inside(r, x, y, 0.0)) {
        return Some("panel");
    }
    let (pad, name) = if dragging { (HALO, "halo") } else { (0.0, "pet") };
    near_pet(regions, x, y, pad).then_some(name)
}
