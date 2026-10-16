//! Native drop target for the pet.
//!
//! macOS fixes which windows can be drag destinations when a drag starts: a window that stops
//! ignoring the mouse mid-drag only gets the drop at release, and meanwhile the app window under
//! the cursor keeps the hover (Chrome activates itself when a drag rests on it). So the pet gets
//! a small *catcher* panel covering the pet plus its drop halo, below the pet panel, that never
//! ignores the mouse. It is the drag destination from the first moment of every drag. The cost,
//! accepted by the plan, is that clicks within the halo ring around the pet are absorbed.
//!
//! Both the catcher and the pet panel hold a `CopetDropView`. In the pet panel it lies over the
//! webview with `hitTest:` returning nil (clicks still reach the page) and is registered for
//! the dragged types from the start, so AppKit always picks it over WebKit: WebKit never sees a
//! drag and its default drop (navigating to the dropped file) can't happen. The view answers the
//! dragging protocol synchronously: `Copy` inside the halo for drags from other apps, nothing
//! otherwise; on drop it reads the drag pasteboard and starts the capture. Spring-loading is
//! disabled on the view, and since macOS still activates the destination's app when a drag rests
//! on a window, an activation of Co-pet during a drag is handed back to the source app. Co-pet's
//! own tray drag-outs are left alone while they run and hand activation back once at the end.
use std::cell::{Cell, RefCell};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSAutoresizingMaskOptions, NSBackingStoreType,
    NSColor, NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSPanel, NSRunningApplication,
    NSSpringLoadingDestination, NSSpringLoadingOptions, NSView, NSWindow, NSWindowCollectionBehavior,
    NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{NSArray, NSObjectProtocol, NSPoint, NSRect, NSString};
use tauri::AppHandle;

use super::frontmost::{self, FrontApp};
use super::{pasteboard, pointer, screens};
use crate::capture;
use crate::model::Rect;

/// Set when the views are installed; their callbacks have no other way to the app.
/// It needs the runtime handle, so it can't be a `LazyLock`.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// `NSFloatingWindowLevel`, the pet panel's level.
const FLOATING_LEVEL: isize = 4;

thread_local! {
    /// The app the current drag came from, recorded when the drag starts so a later activation
    /// change can't mislabel it. Kept past the drag's end: AppKit may deliver the drop after the
    /// pointer loop has seen the button go up. Main thread only, like everything here.
    static SOURCE: RefCell<Option<FrontApp>> = const { RefCell::new(None) };
    static DRAGGING: Cell<bool> = const { Cell::new(false) };
    /// The source app to hand activation back to, and until when (see `set_dragging`).
    static GUARD: Cell<Option<(i32, Instant)>> = const { Cell::new(None) };
    /// The current drag is Co-pet's own tray drag-out (see `mark_own_drag`).
    static OWN: Cell<bool> = const { Cell::new(false) };
    /// The app that was frontmost when the own drag-out started.
    static OWN_FRONT: Cell<Option<i32>> = const { Cell::new(None) };
    /// After an own drag-out: the app to give activation back to, and from when.
    static OWN_RESTORE: Cell<Option<(i32, Instant)>> = const { Cell::new(None) };
    /// Whether Co-pet was the active app at the last check, so each activation is handled once.
    static WAS_ACTIVE: Cell<bool> = const { Cell::new(false) };
    /// The catcher panel (the pet panel's drop view is retained by its content view).
    static CATCHER: RefCell<Option<Retained<NSPanel>>> = const { RefCell::new(None) };
    /// The last frame given to the catcher and the primary display height it was converted
    /// with. The pointer loop places the catcher every tick and AppKit's stored frame need not
    /// equal the fractional CSS-px rect, so comparing against `frame()` re-set it every tick.
    static CATCHER_FRAME: Cell<Option<(Rect, f64)>> = const { Cell::new(None) };
}

const DRAGGED_TYPES: [&str; 7] = [
    "public.file-url",
    "NSFilenamesPboardType",
    pasteboard::TYPE_URL,
    pasteboard::TYPE_PLAIN,
    pasteboard::TYPE_HTML,
    pasteboard::TYPE_PNG,
    pasteboard::TYPE_TIFF,
];

define_class!(
    // SAFETY: NSView has no subclassing requirements; `DropView` does not implement `Drop`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "CopetDropView"]
    pub struct DropView;

    impl DropView {
        /// Transparent to the mouse: in the pet panel clicks and hovers go to the webview.
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> *mut NSView {
            std::ptr::null_mut()
        }
    }

    unsafe impl NSObjectProtocol for DropView {}

    // SAFETY: the signatures match NSSpringLoadingDestination.
    unsafe impl NSSpringLoadingDestination for DropView {
        #[unsafe(method(springLoadingActivated:draggingInfo:))]
        fn spring_loading_activated(&self, _activated: bool, _info: &ProtocolObject<dyn NSDraggingInfo>) {}

        #[unsafe(method(springLoadingHighlightChanged:))]
        fn spring_loading_highlight_changed(&self, _info: &ProtocolObject<dyn NSDraggingInfo>) {}

        #[unsafe(method(springLoadingEntered:))]
        fn spring_loading_entered(&self, _info: &ProtocolObject<dyn NSDraggingInfo>) -> NSSpringLoadingOptions {
            NSSpringLoadingOptions::Disabled
        }

        #[unsafe(method(springLoadingUpdated:))]
        fn spring_loading_updated(&self, _info: &ProtocolObject<dyn NSDraggingInfo>) -> NSSpringLoadingOptions {
            NSSpringLoadingOptions::Disabled
        }
    }

    // SAFETY: the signatures match NSDraggingDestination.
    unsafe impl NSDraggingDestination for DropView {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            if OWN.get() || sender.draggingSource().is_some() {
                return NSDragOperation::None;
            }
            SOURCE.with_borrow_mut(|s| {
                if s.is_none() {
                    *s = frontmost::frontmost_app();
                }
                arm_guard(s.as_ref());
            });
            self.operation(sender)
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            self.operation(sender)
        }

        #[unsafe(method(prepareForDragOperation:))]
        fn prepare_for_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            self.operation(sender) == NSDragOperation::Copy
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag_operation(&self, _sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let source = SOURCE.take();
            arm_guard(source.as_ref());
            #[cfg(debug_assertions)]
            eprintln!("[drop] performDragOperation source={:?}", source.as_ref().and_then(|s| s.name.as_deref()));
            match (APP.get(), capture::drop::read(source)) {
                (Some(app), Some(dropped)) => {
                    capture::drop::spawn(app, dropped);
                    true
                }
                _ => false,
            }
        }
    }
);

impl DropView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        // SAFETY: `initWithFrame:` is NSView's designated initializer.
        let view: Retained<Self> = unsafe { msg_send![Self::alloc(mtm), initWithFrame: frame] };
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let types: Vec<Retained<NSString>> = DRAGGED_TYPES.iter().map(|t| NSString::from_str(t)).collect();
        view.registerForDraggedTypes(&NSArray::from_retained_slice(&types));
        view
    }

    /// `Copy` while the cursor is inside the halo and the drag comes from another app (our own
    /// tray drag-out has a source in this process).
    fn operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
        let (Some(app), Some(window)) = (APP.get(), self.window()) else { return NSDragOperation::None };
        if sender.draggingSource().is_some() {
            return NSDragOperation::None;
        }
        let screen = window.convertPointToScreen(sender.draggingLocation());
        let (x, y) = screens::point_to_top_left(self.mtm(), screen);
        if pointer::in_drop_halo(app, self.mtm(), x, y) {
            NSDragOperation::Copy
        } else {
            NSDragOperation::None
        }
    }
}

/// Lays the drop view over the pet window's webview and creates the (hidden) catcher panel.
pub fn install(app: &AppHandle, mtm: MainThreadMarker, window: &NSWindow) {
    let _ = APP.set(app.clone());
    let Some(content) = window.contentView() else { return };
    content.addSubview(&DropView::new(mtm, content.bounds()));
    CATCHER.set(Some(new_catcher(mtm)));
}

fn new_catcher(mtm: MainThreadMarker) -> Retained<NSPanel> {
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), objc2_foundation::NSSize::new(1.0, 1.0));
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        rect,
        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        NSBackingStoreType::Buffered,
        false,
    );
    // SAFETY: we keep the panel alive in `CATCHER` and never close it.
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(false);
    panel.setLevel(FLOATING_LEVEL);
    panel.setFloatingPanel(true);
    panel.setHidesOnDeactivate(false);
    panel.setBecomesKeyOnlyIfNeeded(true);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    panel.setIgnoresMouseEvents(false);
    // Like tauri-nspanel for the pet panel: make WindowServer's activation-prevention tag match
    // the nonactivating style, so accepting a drop never activates Co-pet.
    let prevents = objc2::sel!(_setPreventsActivation:);
    // SAFETY: `respondsToSelector:` guards the private selector; it takes one BOOL.
    unsafe {
        let responds: bool = msg_send![&*panel, respondsToSelector: prevents];
        if responds {
            let _: () = msg_send![&*panel, _setPreventsActivation: true];
        }
    }
    panel.setContentView(Some(&DropView::new(mtm, rect)));
    panel
}

/// Puts the catcher over `rect` (global top-left points) just below the pet panel, or hides it.
pub fn place_catcher(mtm: MainThreadMarker, pet: &NSWindow, rect: Option<Rect>) {
    CATCHER.with_borrow(|catcher| {
        let Some(catcher) = catcher else { return };
        match rect {
            None => {
                CATCHER_FRAME.set(None);
                if catcher.isVisible() {
                    catcher.orderOut(None);
                }
            }
            Some(rect) => {
                let key = (rect, screens::primary_height(mtm));
                if CATCHER_FRAME.get() != Some(key) {
                    screens::set_window_frame(mtm, catcher, rect);
                    CATCHER_FRAME.set(Some(key));
                }
                if !catcher.isVisible() {
                    catcher.orderWindow_relativeTo(NSWindowOrderingMode::Below, pet.windowNumber());
                }
            }
        }
    });
}

/// How long after a drag ends Co-pet still gives activation back to the drag's source app.
const GUARD_AFTER: Duration = Duration::from_millis(1500);
/// How recent a `mark_own_drag` must be for the next drag to count as Co-pet's own.
const OWN_MARK_WINDOW: Duration = Duration::from_secs(3);
/// How long after an own drag-out ends activation is given back (lets the drop settle).
const OWN_RESTORE_AFTER: Duration = Duration::from_millis(250);

/// Set by `mark_own_drag` (any thread): when, and the app that was frontmost then.
static OWN_MARK: Mutex<Option<(Instant, Option<i32>)>> = Mutex::new(None);

/// Called when the frontend prepares a tray drag-out, just before it starts the OS drag: the next
/// drag the pointer loop sees is Co-pet's own. Records the user's app, which the non-activating
/// pet panel left frontmost.
pub fn mark_own_drag() {
    let own = std::process::id() as i32;
    let front = frontmost::frontmost_pid().filter(|&pid| pid != own);
    *OWN_MARK.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), front));
}

/// Hands any activation of Co-pet back to `source` until `GUARD_AFTER` from now. Arming a fresh
/// guard treats an already active Co-pet as a new activation.
fn arm_guard(source: Option<&FrontApp>) {
    let own = std::process::id() as i32;
    if let Some(pid) = source.map(|s| s.pid).filter(|&pid| pid != own) {
        if GUARD.replace(Some((pid, Instant::now() + GUARD_AFTER))).is_none() {
            WAS_ACTIVE.set(false);
        }
    }
}

fn begin_drag() {
    let mark = OWN_MARK.lock().unwrap_or_else(|e| e.into_inner()).take();
    match mark.filter(|(at, _)| at.elapsed() < OWN_MARK_WINDOW) {
        Some((_, front)) => {
            OWN.set(true);
            OWN_FRONT.set(front);
            OWN_RESTORE.set(None);
            GUARD.set(None);
            #[cfg(debug_assertions)]
            eprintln!("[drop] own drag-out started (front pid {front:?})");
        }
        None => {
            OWN.set(false);
            SOURCE.set(frontmost::frontmost_app());
        }
    }
}

fn end_drag() {
    if OWN.replace(false) {
        OWN_RESTORE.set(OWN_FRONT.take().map(|pid| (pid, Instant::now() + OWN_RESTORE_AFTER)));
    }
}

fn give_back(pid: i32, why: &str) {
    if let Some(source) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        let ok = source.activateWithOptions(NSApplicationActivationOptions::empty());
        #[cfg(debug_assertions)]
        eprintln!("[drop] {why}: gave activation back to pid {pid}: {ok}");
        let _ = (ok, why);
    }
}

/// Called by the pointer loop every tick. Records the source app when a drag starts, and keeps
/// Co-pet from taking over: when a drag rests on the catcher, macOS activates the window's app
/// (the same hover activation Chrome does), which for the accessory Co-pet means losing the
/// user's app. While a drag is on and briefly after, each activation of Co-pet is handed back to
/// the source app once, unless Home or Onboarding (`may_restore` false) is open. Co-pet's own
/// drag-outs are not fought tick by tick: once one ends, activation goes back once to the app
/// that was frontmost when it started.
pub fn set_dragging(mtm: MainThreadMarker, dragging: bool, may_restore: bool) {
    if DRAGGING.replace(dragging) != dragging {
        if dragging {
            begin_drag();
        } else {
            end_drag();
        }
    }
    if dragging && !OWN.get() {
        SOURCE.with_borrow(|s| arm_guard(s.as_ref()));
    }
    let active = NSApplication::sharedApplication(mtm).isActive();
    let became_active = active && !WAS_ACTIVE.replace(active);
    if OWN.get() {
        return;
    }
    if let Some((pid, at)) = OWN_RESTORE.get().filter(|_| !dragging) {
        if Instant::now() >= at {
            OWN_RESTORE.set(None);
            if may_restore && active {
                give_back(pid, "drag-out ended");
            }
        }
    }
    let Some((pid, until)) = GUARD.get() else { return };
    if Instant::now() >= until {
        GUARD.set(None);
        return;
    }
    if may_restore && became_active {
        give_back(pid, "drag");
    }
}
