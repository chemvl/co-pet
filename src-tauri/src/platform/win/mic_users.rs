//! Which processes capture from the default microphone: WASAPI audio sessions on the default
//! capture endpoint (`IAudioSessionManager2`), active state only. Changes are polled by the call
//! watch (`calls::CHECK_EVERY_MS`); `listen` adds session-created notifications for prompt starts.
//!
//! Both join the MTA: `read` must never run on the main thread (it is an STA for tao/WebView2);
//! `listen` may be called from anywhere, since it registers on its own thread.
use std::sync::mpsc::{channel, Sender};

use windows::core::{implement, Interface, Ref};
use windows::Win32::Foundation::S_OK;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MicUser {
    pub pid: i32,
    /// Lowercase exe name (bundle id on macOS).
    pub bundle_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MicUsers {
    /// Who has an active capture session (Co-pet's own pid already removed).
    Processes(Vec<MicUser>),
    /// Older macOS only; Windows always knows the processes.
    #[allow(dead_code)]
    DeviceOnly { running: bool },
}

fn manager() -> windows::core::Result<IAudioSessionManager2> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = e
            .GetDefaultAudioEndpoint(eCapture, eCommunications)
            .or_else(|_| e.GetDefaultAudioEndpoint(eCapture, eConsole))?;
        dev.Activate(CLSCTX_ALL, None)
    }
}

pub fn read() -> MicUsers {
    let own = std::process::id();
    let users = (|| -> windows::core::Result<Vec<MicUser>> {
        let list = unsafe { manager()?.GetSessionEnumerator()? };
        let mut out = Vec::new();
        for i in 0..unsafe { list.GetCount()? } {
            let ctl: IAudioSessionControl2 = unsafe { list.GetSession(i)?.cast()? };
            if unsafe { ctl.GetState()? } != AudioSessionStateActive || unsafe { ctl.IsSystemSoundsSession() } == S_OK {
                continue;
            }
            let pid = unsafe { ctl.GetProcessId()? };
            if pid == 0 || pid == own {
                continue;
            }
            if let Some(id) = super::exe_path(pid).as_deref().and_then(super::app_id_of) {
                out.push(MicUser { pid: pid as i32, bundle_id: id });
            }
        }
        Ok(out)
    })();
    match users {
        Ok(u) => MicUsers::Processes(u),
        Err(e) => {
            eprintln!("[calls] mic sessions: {e}");
            MicUsers::Processes(Vec::new())
        }
    }
}

#[implement(IAudioSessionNotification)]
struct OnSession(Box<dyn Fn() + Send + Sync>);

impl IAudioSessionNotification_Impl for OnSession_Impl {
    fn OnSessionCreated(&self, _s: Ref<IAudioSessionControl>) -> windows::core::Result<()> {
        (self.0)();
        Ok(())
    }
}

/// Owns the MTA thread that holds the session notification; dropping it unregisters.
pub struct Listening {
    /// Dropping it disconnects the thread's receiver: the thread unregisters and exits.
    _stop: Sender<()>,
}

impl Listening {
    /// Sessions are re-enumerated on every `read`; nothing to refresh.
    pub fn refresh_processes(&mut self) {}
}

/// Registers for new sessions on its own MTA thread: session notifications reach only MTA
/// threads, and the main thread must stay an STA. `on_change` runs on a COM worker thread and
/// must only notify.
pub fn listen(on_change: Box<dyn Fn() + Send + Sync>) -> Listening {
    let (stop, stopped) = channel::<()>();
    let spawned = std::thread::Builder::new().name("mic-sessions".into()).spawn(move || {
        let registered = manager().and_then(|m| {
            let sink: IAudioSessionNotification = OnSession(on_change).into();
            unsafe {
                m.RegisterSessionNotification(&sink)?;
                // The first enumeration after registering is what arms notifications.
                let _ = m.GetSessionEnumerator();
            }
            Ok((m, sink))
        });
        match registered {
            Ok((m, sink)) => {
                // Blocks until `Listening` drops.
                let _ = stopped.recv();
                let _ = unsafe { m.UnregisterSessionNotification(&sink) };
                drop((m, sink));
            }
            Err(e) => eprintln!("[calls] mic listen: {e}"),
        }
        // Balances `manager()`'s `CoInitializeEx` (this thread joined the MTA there).
        unsafe { CoUninitialize() };
    });
    if let Err(e) = spawned {
        eprintln!("[calls] mic listen thread: {e}");
    }
    Listening { _stop: stop }
}
