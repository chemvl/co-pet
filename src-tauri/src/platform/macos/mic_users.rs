//! Which processes have microphone input running, read from CoreAudio with no permission.
//!
//! Newer macOS exposes one audio object per client process (`ProcessObjectList`), each with its
//! pid, bundle id and an `IsRunningInput` flag. Older macOS lacks them (`'who?'`), so [`read`]
//! falls back to whether the default input device runs for anyone, Co-pet included.
//!
//! [`listen`] registers property listeners so callers wake only when mic use changes: no polling.
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2_foundation::NSString;

/// `kAudioObjectSystemObject`.
const SYSTEM_OBJECT: u32 = 1;
/// `kAudioObjectUnknown`: no device.
const NO_OBJECT: u32 = 0;
/// `kAudioObjectPropertyScopeGlobal`.
const SCOPE_GLOBAL: u32 = fourcc(b"glob");
/// `kAudioObjectPropertyElementMain`.
const ELEMENT_MAIN: u32 = 0;
/// `kAudioHardwarePropertyDefaultInputDevice` (system object).
const DEFAULT_INPUT: u32 = fourcc(b"dIn ");
/// `kAudioDevicePropertyDeviceIsRunningSomewhere` (device).
const RUNNING_SOMEWHERE: u32 = fourcc(b"gone");
/// `kAudioHardwarePropertyProcessObjectList` (system object).
const PROCESS_LIST: u32 = fourcc(b"prs#");
/// `kAudioProcessPropertyPID` (process object).
const PROCESS_PID: u32 = fourcc(b"ppid");
/// `kAudioProcessPropertyBundleID` (process object), a CFString the caller releases.
const PROCESS_BUNDLE_ID: u32 = fourcc(b"pbid");
/// `kAudioProcessPropertyIsRunningInput` (process object).
const PROCESS_RUNNING_INPUT: u32 = fourcc(b"piri");
/// `kAudioHardwareUnknownPropertyError`: the property doesn't exist on this OS.
const UNKNOWN_PROPERTY: i32 = fourcc(b"who?") as i32;

const fn fourcc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AudioObjectPropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

const fn address(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress { selector, scope: SCOPE_GLOBAL, element: ELEMENT_MAIN }
}

type ListenerProc =
    unsafe extern "C" fn(u32, u32, *const AudioObjectPropertyAddress, *mut c_void) -> i32;

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectGetPropertyDataSize(
        object: u32,
        address: *const AudioObjectPropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        data_size: *mut u32,
    ) -> i32;
    fn AudioObjectGetPropertyData(
        object: u32,
        address: *const AudioObjectPropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        data_size: *mut u32,
        data: *mut c_void,
    ) -> i32;
    fn AudioObjectAddPropertyListener(
        object: u32,
        address: *const AudioObjectPropertyAddress,
        listener: ListenerProc,
        client_data: *mut c_void,
    ) -> i32;
    fn AudioObjectRemovePropertyListener(
        object: u32,
        address: *const AudioObjectPropertyAddress,
        listener: ListenerProc,
        client_data: *mut c_void,
    ) -> i32;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MicUser {
    pub pid: i32,
    /// Empty when the process has none.
    pub bundle_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MicUsers {
    /// Process objects available: who has input running (Co-pet's own pid already removed).
    Processes(Vec<MicUser>),
    /// Older macOS: only whether the default input runs for anyone (Co-pet included).
    DeviceOnly { running: bool },
}

pub fn read() -> MicUsers {
    match process_objects() {
        Ok(objects) => {
            let users = objects.into_iter().filter_map(mic_user).collect();
            MicUsers::Processes(without_self(users, std::process::id() as i32))
        }
        Err(UNKNOWN_PROPERTY) => {
            let running = default_input().is_some_and(|d| get::<u32>(d, RUNNING_SOMEWHERE) == Ok(1));
            MicUsers::DeviceOnly { running }
        }
        // Process objects exist but the list couldn't be read right now: nobody known.
        Err(_) => MicUsers::Processes(Vec::new()),
    }
}

/// The process behind `object` if its input runs.
fn mic_user(object: u32) -> Option<MicUser> {
    if get::<u32>(object, PROCESS_RUNNING_INPUT).ok()? == 0 {
        return None;
    }
    let pid = get::<i32>(object, PROCESS_PID).ok()?;
    Some(MicUser { pid, bundle_id: bundle_id(object).unwrap_or_default() })
}

fn bundle_id(object: u32) -> Option<String> {
    let raw = get::<*mut c_void>(object, PROCESS_BUNDLE_ID).ok()?;
    // SAFETY: the property is a CFString returned +1 (released by `Retained`), toll-free bridged
    // to NSString; null when the process has no bundle id.
    let string = unsafe { Retained::from_raw(raw.cast::<NSString>()) }?;
    Some(string.to_string())
}

fn without_self(users: Vec<MicUser>, me: i32) -> Vec<MicUser> {
    users.into_iter().filter(|u| u.pid != me).collect()
}

fn default_input() -> Option<u32> {
    get::<u32>(SYSTEM_OBJECT, DEFAULT_INPUT).ok().filter(|&d| d != NO_OBJECT)
}

/// `PROCESS_LIST` of the system object; `Err(UNKNOWN_PROPERTY)` on an OS without process objects.
fn process_objects() -> Result<Vec<u32>, i32> {
    let addr = address(PROCESS_LIST);
    let mut size = 0u32;
    // SAFETY: valid address and out-pointer; no qualifier.
    let status = unsafe { AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &addr, 0, std::ptr::null(), &mut size) };
    if status != 0 {
        return Err(status);
    }
    let mut objects = vec![NO_OBJECT; size as usize / size_of::<u32>()];
    let mut size = (objects.len() * size_of::<u32>()) as u32;
    // SAFETY: `objects` holds `size` bytes; CoreAudio writes at most that and reports what it wrote.
    let status = unsafe {
        AudioObjectGetPropertyData(SYSTEM_OBJECT, &addr, 0, std::ptr::null(), &mut size, objects.as_mut_ptr().cast())
    };
    if status != 0 {
        return Err(status);
    }
    objects.truncate(size as usize / size_of::<u32>());
    Ok(objects)
}

/// A fixed-size property (`u32`, `i32` or a CF object pointer).
fn get<T: Copy>(object: u32, selector: u32) -> Result<T, i32> {
    let mut value = MaybeUninit::<T>::uninit();
    let mut size = size_of::<T>() as u32;
    // SAFETY: `value` holds `size` bytes, the size CoreAudio documents for these properties.
    let status = unsafe {
        AudioObjectGetPropertyData(object, &address(selector), 0, std::ptr::null(), &mut size, value.as_mut_ptr().cast())
    };
    if status != 0 || size as usize != size_of::<T>() {
        return Err(status);
    }
    // SAFETY: CoreAudio wrote all `size_of::<T>()` bytes.
    Ok(unsafe { value.assume_init() })
}

/// Shared with CoreAudio's notification thread through the listeners' client data.
struct Context {
    on_change: Box<dyn Fn() + Send + Sync>,
    /// The device carrying the `RUNNING_SOMEWHERE` listener (`NO_OBJECT` when none); `None` once
    /// `Drop` closed it, so a late `DEFAULT_INPUT` callback can't register a listener again.
    input: Mutex<Option<u32>>,
}

impl Context {
    fn client_data(&self) -> *mut c_void {
        (self as *const Context).cast_mut().cast()
    }

    /// Moves the `RUNNING_SOMEWHERE` listener to the current default input device, unless closed.
    fn retarget_input(&self) {
        let mut input = self.input.lock().unwrap_or_else(|e| e.into_inner());
        let Some(current) = input.as_mut() else { return };
        if *current != NO_OBJECT {
            remove(*current, RUNNING_SOMEWHERE, on_property, self.client_data());
        }
        *current = default_input()
            .filter(|&d| add(d, RUNNING_SOMEWHERE, on_property, self.client_data()))
            .unwrap_or(NO_OBJECT);
    }

    fn notify(&self) {
        let _ = catch_unwind(AssertUnwindSafe(|| (self.on_change)()));
    }
}

/// Live CoreAudio listeners; removed on drop.
pub struct Listening {
    /// `Box::into_raw` of the context every listener gets as client data. Never reclaimed: see
    /// `Drop`.
    context: *mut Context,
    /// System-object registrations: `DEFAULT_INPUT` and, when supported, `PROCESS_LIST`.
    system: Vec<(u32, ListenerProc)>,
    /// Process objects carrying an `IsRunningInput` listener; `None` without process objects.
    processes: Option<Vec<u32>>,
}

// SAFETY: `context` is never freed and `Context` is `Send + Sync`; CoreAudio's listener functions
// are thread-safe.
unsafe impl Send for Listening {}

/// Registers listeners; `on_change` runs on CoreAudio's thread and must only send on a channel.
pub fn listen(on_change: Box<dyn Fn() + Send + Sync>) -> Listening {
    let context = Box::into_raw(Box::new(Context { on_change, input: Mutex::new(Some(NO_OBJECT)) }));
    // SAFETY: just allocated and never freed.
    let ctx = unsafe { &*context };
    let mut system = Vec::new();
    if add(SYSTEM_OBJECT, DEFAULT_INPUT, on_default_input, ctx.client_data()) {
        system.push((DEFAULT_INPUT, on_default_input as ListenerProc));
    }
    ctx.retarget_input();
    let mut listening = Listening { context, system, processes: None };
    if process_objects().is_ok() && add(SYSTEM_OBJECT, PROCESS_LIST, on_property, ctx.client_data()) {
        listening.system.push((PROCESS_LIST, on_property as ListenerProc));
        listening.processes = Some(Vec::new());
        listening.refresh_processes();
    }
    listening
}

impl Listening {
    /// Re-registers per-process `IsRunningInput` listeners after `PROCESS_LIST` changed.
    pub fn refresh_processes(&mut self) {
        let Some(current) = self.processes.as_mut() else { return };
        let Ok(objects) = process_objects() else { return };
        // SAFETY: `context` is never freed.
        let data = unsafe { &*self.context }.client_data();
        for &gone in current.iter().filter(|o| !objects.contains(o)) {
            remove(gone, PROCESS_RUNNING_INPUT, on_property, data);
        }
        current.retain(|o| objects.contains(o));
        for object in objects {
            if !current.contains(&object) && add(object, PROCESS_RUNNING_INPUT, on_property, data) {
                current.push(object);
            }
        }
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        // SAFETY: `context` is never freed.
        let ctx = unsafe { &*self.context };
        let data = ctx.client_data();
        // `DEFAULT_INPUT` first, so no new callback re-targets the device listener meanwhile.
        for &(selector, proc) in &self.system {
            remove(SYSTEM_OBJECT, selector, proc, data);
        }
        // Close under the lock: a callback already in flight then finds `None` and adds nothing.
        let closed = ctx.input.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(device) = closed.filter(|&d| d != NO_OBJECT) {
            remove(device, RUNNING_SOMEWHERE, on_property, data);
        }
        for &object in self.processes.iter().flatten() {
            remove(object, PROCESS_RUNNING_INPUT, on_property, data);
        }
        // The context is never freed: CoreAudio doesn't promise that removing a listener waits
        // for a callback already running on its thread, which may still read it. One small
        // context leaks per `transcribe.autoRecord` switch to `off`.
    }
}

fn add(object: u32, selector: u32, proc: ListenerProc, data: *mut c_void) -> bool {
    // SAFETY: valid address; `data` points at a leaked, never-freed `Context`.
    unsafe { AudioObjectAddPropertyListener(object, &address(selector), proc, data) == 0 }
}

fn remove(object: u32, selector: u32, proc: ListenerProc, data: *mut c_void) {
    // SAFETY: valid address. Fails harmlessly when the object is already gone.
    unsafe { AudioObjectRemovePropertyListener(object, &address(selector), proc, data) };
}

/// `RUNNING_SOMEWHERE`, `PROCESS_LIST` and `PROCESS_RUNNING_INPUT` changes.
unsafe extern "C" fn on_property(
    _object: u32,
    _count: u32,
    _addresses: *const AudioObjectPropertyAddress,
    data: *mut c_void,
) -> i32 {
    // SAFETY: `data` is the leaked `Context` registered with this listener; it is never freed.
    let ctx = unsafe { &*data.cast::<Context>() };
    ctx.notify();
    0
}

/// The default input device changed: follow it, then notify.
unsafe extern "C" fn on_default_input(
    _object: u32,
    _count: u32,
    _addresses: *const AudioObjectPropertyAddress,
    data: *mut c_void,
) -> i32 {
    // SAFETY: as in `on_property`.
    let ctx = unsafe { &*data.cast::<Context>() };
    let _ = catch_unwind(AssertUnwindSafe(|| ctx.retarget_input()));
    ctx.notify();
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(pid: i32, bundle_id: &str) -> MicUser {
        MicUser { pid, bundle_id: bundle_id.into() }
    }

    #[test]
    fn fourcc_packs_big_endian() {
        assert_eq!(fourcc(b"prs#"), 0x70727323);
        assert_eq!(fourcc(b"dIn "), 0x64496e20);
    }

    #[test]
    fn without_self_drops_own_pid_and_keeps_order() {
        let users = vec![user(3, "us.zoom.xos"), user(7, "copet"), user(5, "com.apple.replayd"), user(7, "")];
        assert_eq!(without_self(users, 7), vec![user(3, "us.zoom.xos"), user(5, "com.apple.replayd")]);
    }
}
