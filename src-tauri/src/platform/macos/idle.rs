//! Sleep (Step 10.4): the pet sleeps after 5 minutes without any input and during the user's
//! sleep hours (`pet.sleepHours`, may wrap midnight). Polled every 5 s; `pet-flags {sleep}` is
//! emitted on change.
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use chrono::Timelike;
use serde::Deserialize;
use tauri::{AppHandle, Manager};

use crate::db::settings;
use crate::events::{self, PetFlags};

const TICK: Duration = Duration::from_secs(5);
const IDLE_SECS: f64 = 300.0;
/// `kCGEventSourceStateHIDSystemState`.
const HID_SYSTEM_STATE: i32 = 1;
/// `kCGAnyInputEventType`.
const ANY_INPUT_EVENT: u32 = !0;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state_id: i32, event_type: u32) -> f64;
}

#[derive(Deserialize)]
struct SleepHours {
    start: String,
    end: String,
}

/// Managed: the last sleep flag sent to the pet.
#[derive(Default)]
pub struct Sleep(Mutex<Option<bool>>);

impl Sleep {
    fn lock(&self) -> MutexGuard<'_, Option<bool>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn current(&self) -> bool {
        self.lock().unwrap_or(false)
    }
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            // SAFETY: plain C call with constant arguments.
            let idle = unsafe { CGEventSourceSecondsSinceLastEventType(HID_SYSTEM_STATE, ANY_INPUT_EVENT) };
            let sleep = idle >= IDLE_SECS || in_sleep_hours(&app);
            let state = app.state::<Sleep>();
            let mut last = state.lock();
            if *last != Some(sleep) {
                *last = Some(sleep);
                drop(last);
                eprintln!("[idle] sleep={sleep}");
                events::pet_flags(&app, PetFlags { sleep: Some(sleep), ..Default::default() });
            }
        }
    });
}

fn in_sleep_hours(app: &AppHandle) -> bool {
    let Ok(hours) = settings::read::<SleepHours>(app, "pet.sleepHours") else { return false };
    let (Some(start), Some(end)) = (minutes(&hours.start), minutes(&hours.end)) else { return false };
    let now = chrono::Local::now();
    let now = now.hour() * 60 + now.minute();
    if start <= end {
        start <= now && now < end
    } else {
        now >= start || now < end
    }
}

/// `HH:MM` → minutes after midnight.
fn minutes(hhmm: &str) -> Option<u32> {
    let (h, m) = hhmm.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}
