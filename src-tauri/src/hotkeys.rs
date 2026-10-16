//! Global hotkeys (Step 10.2), registered from the `hotkeys` setting. A press of `focusPet` or
//! `quickNote` makes the pet panel key first; every press then emits `hotkey {action}` and the pet
//! frontend runs the action.
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use serde_json::{Map, Value};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::db::settings;
use crate::error::{AppError, AppResult};
use crate::events::{self, Hotkey};
use crate::platform::macos::fullscreen;
use crate::windows;

const KEY: &str = "hotkeys";
const ACTIONS: [&str; 4] = ["focusPet", "sendSelection", "captureScreen", "quickNote"];

/// Managed: the shortcut each action currently holds.
#[derive(Default)]
pub struct Hotkeys(Mutex<BTreeMap<&'static str, Shortcut>>);

impl Hotkeys {
    fn lock(&self) -> MutexGuard<'_, BTreeMap<&'static str, Shortcut>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn action_of(name: &str) -> Option<&'static str> {
    ACTIONS.iter().copied().find(|a| *a == name)
}

fn register(app: &AppHandle, action: &'static str, shortcut: Shortcut) -> AppResult<()> {
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _, event| {
            if event.state == ShortcutState::Pressed {
                fire(app, action);
            }
        })
        .map_err(|e| AppError::msg(e.to_string()))
}

fn fire(app: &AppHandle, action: &'static str) {
    #[cfg(debug_assertions)]
    eprintln!("[hotkeys] pressed {action}");
    if matches!(action, "focusPet" | "quickNote") {
        if action == "focusPet" {
            // The focus hotkey brings a "Hide for 1 hour" pet back early.
            fullscreen::show_now(app);
        }
        if let Err(e) = windows::focus_pet_panel(app) {
            eprintln!("[hotkeys] {action}: {e}");
        }
    }
    events::emit(app, events::HOTKEY, Hotkey { action: action.to_string() });
}

/// Registers every saved hotkey. A shortcut another app holds is logged and left unregistered;
/// Settings shows it and lets the user pick another.
pub fn init(app: &AppHandle) {
    let saved: Map<String, Value> = settings::read(app, KEY).unwrap_or_default();
    let state = app.state::<Hotkeys>();
    let mut map = state.lock();
    for action in ACTIONS {
        let Some(acc) = saved.get(action).and_then(Value::as_str) else { continue };
        let shortcut = match acc.parse::<Shortcut>() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[hotkeys] {action}: can't parse {acc:?}: {e}");
                continue;
            }
        };
        match register(app, action, shortcut) {
            Ok(()) => {
                map.insert(action, shortcut);
            }
            Err(e) => eprintln!("[hotkeys] {action}: {acc} not registered: {e}"),
        }
    }
}

/// Moves `action` to `accelerator`. Errors: `invalid` (unknown action or unparsable accelerator),
/// `inUse` (another action or another app holds it; the old shortcut is restored).
pub fn set_hotkey(app: &AppHandle, action: &str, accelerator: &str) -> AppResult<()> {
    let action = action_of(action).ok_or(AppError::Code("invalid"))?;
    let next: Shortcut = accelerator.parse().map_err(|_| AppError::Code("invalid"))?;
    {
        let state = app.state::<Hotkeys>();
        let mut map = state.lock();
        let old = map.get(action).copied();
        if old != Some(next) {
            if map.iter().any(|(a, s)| *a != action && *s == next) {
                return Err(AppError::Code("inUse"));
            }
            if let Some(old) = old {
                let _ = app.global_shortcut().unregister(old);
                map.remove(action);
            }
            if let Err(e) = register(app, action, next) {
                eprintln!("[hotkeys] {action}: {accelerator} refused: {e}");
                if let Some(old) = old {
                    match register(app, action, old) {
                        Ok(()) => {
                            map.insert(action, old);
                        }
                        Err(e) => eprintln!("[hotkeys] {action}: couldn't restore the old shortcut: {e}"),
                    }
                }
                return Err(AppError::Code("inUse"));
            }
            map.insert(action, next);
        }
    }
    let mut saved: Map<String, Value> = settings::read(app, KEY)?;
    saved.insert(action.to_string(), Value::String(accelerator.to_string()));
    settings::set(app, KEY, &Value::Object(saved))
}
