//! Audio conversion through macOS's `afconvert`. Blocking; call inside `spawn_blocking`.
use std::path::Path;
use std::process::Command;

use crate::error::{AppError, AppResult};

/// 16-bit mono WAV at 16 kHz, the speech engines' input. Failure is `audioConvert`.
pub fn wav16k(input: &Path, output: &Path) -> AppResult<()> {
    afconvert(input, output, Target::Wav16k)
}

/// 32 kbps mono AAC in an `.m4a`, the archived audio. Failure is `audioConvert`.
pub fn aac(input: &Path, output: &Path) -> AppResult<()> {
    afconvert(input, output, Target::Aac)
}

/// What [`afconvert`] produces.
#[derive(Clone, Copy, Debug)]
enum Target {
    /// 16-bit mono WAV at 16 kHz, the speech engines' input.
    Wav16k,
    /// 32 kbps mono AAC in an `.m4a`, the archived audio.
    Aac,
}

/// Runs `/usr/bin/afconvert` (blocking; call it inside `spawn_blocking`). A non-zero exit or a
/// missing output is `audioConvert`.
fn afconvert(input: &Path, output: &Path, to: Target) -> AppResult<()> {
    let args: &[&str] = match to {
        Target::Wav16k => &["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"],
        Target::Aac => &["-f", "m4af", "-d", "aac", "-b", "32000", "-c", "1"],
    };
    match Command::new("/usr/bin/afconvert").args(args).arg(input).arg(output).output() {
        Ok(out) if out.status.success() && output.is_file() => Ok(()),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            eprintln!("[transcribe] afconvert {} ({}): {}", input.display(), out.status, stderr.trim());
            Err(AppError::Code("audioConvert"))
        }
        Err(e) => {
            eprintln!("[transcribe] afconvert {}: {e}", input.display());
            Err(AppError::Code("audioConvert"))
        }
    }
}
