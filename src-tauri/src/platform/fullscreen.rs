//! Hidden states (Step 10.3). The pet hides while the frontmost app has a full-screen window on the
//! pet's monitor, and while the user's "Hide for 1 hour" (`pet.hiddenUntil`) runs. Polled once a
//! second; `pet-visibility` is emitted on change and the pointer loop stops hit-testing while hidden.
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Manager};

use super::os::fullscreen::frontmost_fullscreen_on;
use super::{on_main, pointer::PointerState};
use crate::db::{now_ms, settings};
use crate::events::{self, PetVisibility};
use crate::windows;

const TICK: Duration = Duration::from_secs(1);
const HIDDEN_UNTIL: &str = "pet.hiddenUntil";

/// Managed: the last visibility sent to the pet (`None` until the first poll with a pet window).
#[derive(Default)]
pub struct Visibility(Mutex<Option<PetVisibility>>);

impl Visibility {
    fn lock(&self) -> MutexGuard<'_, Option<PetVisibility>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn current(&self) -> PetVisibility {
        self.lock().clone().unwrap_or(VISIBLE)
    }
}

const VISIBLE: PetVisibility = PetVisibility { hidden: false, reason: None };

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let Some(frame) = windows::pet_screen_frame(&app) else { continue };
            let user = user_hidden(&app);
            // A user hide wins over full screen, so the window list isn't needed meanwhile.
            let full = user
                || match on_main(&app, move |mt| frontmost_fullscreen_on(mt, frame)).await {
                    Ok(full) => full,
                    Err(_) => break, // the event loop is gone: the app is exiting
                };
            let next = match (user, full) {
                (true, _) => PetVisibility { hidden: true, reason: Some("user") },
                (false, true) => PetVisibility { hidden: true, reason: Some("fullscreen") },
                (false, false) => VISIBLE,
            };
            update(&app, next);
        }
    });
}

/// "Show pet" and the focus hotkey: ends a user hide now instead of at the next poll. A
/// full-screen hide stays (the next poll re-evaluates it).
pub fn show_now(app: &AppHandle) {
    let until = settings::read::<Value>(app, HIDDEN_UNTIL).unwrap_or(Value::Null);
    if !until.is_null() {
        if let Err(e) = settings::set(app, HIDDEN_UNTIL, &Value::Null) {
            eprintln!("[visibility] clear {HIDDEN_UNTIL}: {e}");
        }
    }
    let user_hidden = app.state::<Visibility>().current().reason == Some("user");
    if user_hidden {
        update(app, VISIBLE);
    }
}

/// `now < pet.hiddenUntil`; a passed deadline is cleared.
fn user_hidden(app: &AppHandle) -> bool {
    let Ok(Some(until)) = settings::read::<Option<f64>>(app, HIDDEN_UNTIL) else { return false };
    if (now_ms() as f64) < until {
        return true;
    }
    if let Err(e) = settings::set(app, HIDDEN_UNTIL, &Value::Null) {
        eprintln!("[visibility] clear {HIDDEN_UNTIL}: {e}");
    }
    false
}

fn update(app: &AppHandle, next: PetVisibility) {
    let state = app.state::<Visibility>();
    let mut last = state.lock();
    if last.as_ref() == Some(&next) {
        return;
    }
    eprintln!("[visibility] hidden={} reason={:?}", next.hidden, next.reason);
    app.state::<PointerState>().set_hidden(next.hidden);
    events::emit(app, events::PET_VISIBILITY, next.clone());
    *last = Some(next);
}
