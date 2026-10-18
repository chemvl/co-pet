//! Click-through for the pet window. The window covers the whole work area but only its hit
//! regions take the mouse: a 50 ms loop hit-tests the cursor against the regions the frontend
//! reports and flips mouse-ignoring on transitions. `zone` regions are reported without taking the
//! mouse, and a hit panel's `id` is reported as `target` (the webview's own hover goes stale when
//! the mouse is ignored in between). It also detects OS drags (the backend's `Probe`) so the pet
//! region grows into the drop halo, and keeps the native drop target's catcher over the pet and
//! its halo.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use super::os::pointer::{after_tick, left_button_down, Probe};
use super::{pet_window, screens, MainThread};
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
    /// OS drag detection state.
    probe: Probe,
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
                if let Some(mt) = MainThread::new() {
                    tick(&handle, mt);
                }
                handle.state::<PointerState>().queued.store(false, Ordering::Release);
            });
            if queued.is_err() {
                break; // the event loop is gone: the app is exiting
            }
        }
    });
}

fn tick(app: &AppHandle, mt: MainThread) {
    let Some(window) = windows::pet_window(app) else { return };
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
        windows::refresh_screens(app, mt);
    }
    // Read before locking the pointer state: it takes the placement lock.
    let scale = css_scale(app);

    let mut s = state.lock();
    if s.hidden {
        if !s.passthrough {
            pet_window::set_ignores_mouse(mt, &window, true);
            s.passthrough = true;
        }
        emit_if_changed(app, &mut s, None, None, false);
        s.parked = true;
        drop(s);
        after_tick(mt, &window, None, false, windows_closed(app));
        return;
    }

    let left_down = left_button_down();
    let (mx, my) = screens::mouse_location(mt);
    let frame = pet_window::frame(mt, &window);
    let local = screens::contains(&frame, mx, my).then(|| ((mx - frame.x) / scale, (my - frame.y) / scale));
    let over = local.is_some_and(|(x, y)| takes_mouse(hit(&s.regions, x, y, false).0));
    let dragging = s.probe.update(left_down, (mx, my), over);
    let (region, target) = local.map_or((None, None), |(x, y)| hit(&s.regions, x, y, dragging));

    if s.moving {
        drop(s);
        windows::follow_pointer_screen(app, mt, mx, my);
        s = state.lock();
    }

    let inside = takes_mouse(region) || s.moving;
    if s.passthrough == inside {
        pet_window::set_ignores_mouse(mt, &window, !inside);
        s.passthrough = !inside;
    }
    emit_if_changed(app, &mut s, region, target, dragging);
    // The catcher covers pet + halo in global units; it steps aside while the pet is moved.
    let catcher = if s.moving { None } else { pet_bounds(&s.regions, HALO) };
    drop(s);
    let catcher =
        catcher.map(|r| Rect { x: frame.x + r.x * scale, y: frame.y + r.y * scale, w: r.w * scale, h: r.h * scale });
    after_tick(mt, &window, catcher, dragging, windows_closed(app));
}

/// Global units per CSS px on the pet's monitor (see `screens::css_scale`).
fn css_scale(app: &AppHandle) -> f64 {
    windows::current_screen(app).map_or(1.0, |s| screens::css_scale(&s))
}

/// Neither Home nor Onboarding is open: Co-pet has no reason to be the active app.
fn windows_closed(app: &AppHandle) -> bool {
    app.get_webview_window(windows::HOME).is_none() && app.get_webview_window(windows::ONBOARDING).is_none()
}

/// Whether a global top-left point is within the drop halo of the pet.
pub(crate) fn in_drop_halo(app: &AppHandle, mt: MainThread, x: f64, y: f64) -> bool {
    let Some(window) = windows::pet_window(app) else { return false };
    let frame = pet_window::frame(mt, &window);
    let scale = css_scale(app);
    let state = app.state::<PointerState>();
    let s = state.lock();
    !s.hidden && near_pet(&s.regions, (x - frame.x) / scale, (y - frame.y) / scale, HALO)
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

/// Emits `pointer-region` when the region, the hit panel's `id` or dragging changed. `target`
/// indexes `s.regions`.
fn emit_if_changed(app: &AppHandle, s: &mut Inner, region: Option<&'static str>, target: Option<usize>, dragging: bool) {
    let id = target.and_then(|i| s.regions.get(i)).and_then(|r| r.id.as_deref());
    if s.last.as_ref().is_some_and(|l| l.region == region && l.target.as_deref() == id && l.dragging == dragging) {
        return;
    }
    let next = PointerRegion { region, target: id.map(str::to_owned), dragging };
    #[cfg(debug_assertions)]
    eprintln!("[pointer] region={:?} target={:?} dragging={}", next.region, next.target, next.dragging);
    events::emit(app, events::POINTER_REGION, next.clone());
    s.last = Some(next);
}

/// Panels win over the pet, the pet over a zone; while dragging the pet grows by the halo and
/// reports `halo`. A panel hit also returns that region's index.
fn hit(regions: &[HitRegion], x: f64, y: f64, dragging: bool) -> (Option<&'static str>, Option<usize>) {
    if let Some(i) = regions.iter().position(|r| r.kind == "panel" && inside(r, x, y, 0.0)) {
        return (Some("panel"), Some(i));
    }
    let (pad, name) = if dragging { (HALO, "halo") } else { (0.0, "pet") };
    if near_pet(regions, x, y, pad) {
        return (Some(name), None);
    }
    let zone = regions.iter().any(|r| r.kind == "zone" && inside(r, x, y, 0.0));
    (zone.then_some("zone"), None)
}

/// Every region but a zone takes the mouse.
fn takes_mouse(region: Option<&str>) -> bool {
    region.is_some_and(|r| r != "zone")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(x: f64, y: f64, w: f64, h: f64, kind: &str, id: Option<&str>) -> HitRegion {
        HitRegion { x, y, w, h, kind: kind.into(), id: id.map(Into::into) }
    }

    #[test]
    fn bubbles_report_their_index_and_the_zone_never_takes_the_mouse() {
        let regions = [
            region(0.0, 0.0, 200.0, 200.0, "zone", None),
            region(120.0, 80.0, 80.0, 80.0, "pet", None),
            region(20.0, 20.0, 40.0, 40.0, "panel", Some("home")),
        ];
        assert_eq!(hit(&regions, 30.0, 30.0, false), (Some("panel"), Some(2)));
        assert_eq!(hit(&regions, 150.0, 100.0, false), (Some("pet"), None));
        // Between the pet and a bubble: still hovering, but clicks fall through.
        assert_eq!(hit(&regions, 90.0, 90.0, false), (Some("zone"), None));
        assert!(!takes_mouse(Some("zone")));
        assert!(takes_mouse(Some("panel")) && takes_mouse(Some("pet")));
        assert_eq!(hit(&regions, 300.0, 300.0, false), (None, None));
    }
}
