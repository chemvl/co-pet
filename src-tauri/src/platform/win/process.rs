//! Helper processes live in a kill-on-close Job Object, so they die with Co-pet even when it
//! is killed or ended with `process::exit` (Windows doesn't reparent orphans).
use std::sync::LazyLock;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

struct Job(HANDLE);
// SAFETY: a job handle is a process-wide kernel handle; it is only passed to Win32 calls.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

/// The kill-on-close job; `None` when it can't be created (helpers then outlive a killed Co-pet).
static JOB: LazyLock<Option<Job>> = LazyLock::new(|| unsafe {
    let h = CreateJobObjectW(None, None).ok()?;
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    SetInformationJobObject(
        h,
        JobObjectExtendedLimitInformation,
        (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
        std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
    )
    .ok()?;
    Some(Job(h))
});

/// Spawns without a console window and adds the child to the job (the handle is never closed:
/// it closes when Co-pet exits, which kills the children).
pub fn spawn_child(cmd: &mut tokio::process::Command) -> std::io::Result<tokio::process::Child> {
    cmd.creation_flags(CREATE_NO_WINDOW.0);
    let child = cmd.spawn()?;
    if let (Some(job), Some(raw)) = (JOB.as_ref(), child.raw_handle()) {
        if let Err(e) = unsafe { AssignProcessToJobObject(job.0, HANDLE(raw)) } {
            eprintln!("[process] job assign: {e}");
        }
    }
    Ok(child)
}
